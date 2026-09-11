// D-GAME-ASSET-PIPELINE1: backend-neutral facts for declared asset roots,
// deterministic imports, and checked hot reload.  This module deliberately
// performs no filesystem observation and invokes no importer.  Hosts provide
// watch events and checked importer outcomes; the kernel validates, orders,
// and commits their facts.
//
// Keep source and runtime identities separate.  A source identity describes
// authored bytes under a declared root.  An artifact identity describes a
// checked replacement produced from that source, recipe, tool, and version.

pub const JET_GAME_ASSET_MAX_ROOTS: usize = 64;
pub const JET_GAME_ASSET_MAX_PATH_BYTES: usize = 4096;
pub const JET_GAME_ASSET_MAX_ID_BYTES: usize = 256;
pub const JET_GAME_ASSET_MAX_METADATA_BYTES: usize = 4096;

pub const JET_GAME_ASSET_MAX_DEPENDENCIES: usize = 256;
pub const JET_GAME_ASSET_MAX_EVENTS: usize = 1024;
pub const JET_GAME_ASSET_MAX_SKIPPED: usize = 4096;
pub const JET_GAME_ASSET_MAX_DIAGNOSTICS: usize = 64;
pub const JET_GAME_ASSET_MAX_OUTPUTS: usize = 64;
pub const JET_GAME_ASSET_MAX_TRANSACTION_ITEMS: usize = 128;
pub const JET_GAME_ASSET_MAX_READY_ARTIFACTS: usize = 4096;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetGameAssetPipelineError {
    EmptyIdentity { field: &'static str },
    IdentityTooLong { field: &'static str },
    ControlIdentity { field: &'static str },
    InvalidPath { path: String },
    PathEscape { path: String },
    UndeclaredPath { path: String },
    DuplicateRoot { value: String },
    RootNotDeclared { id: String },
    RootMismatch { expected: String, actual: String },
    TooManyRoots,
    TooManyDependencies,
    TooManyEvents,
    TooManyDiagnostics,
    TooManyOutputs,
    TooManyTransactionItems,
    EmptyHash { field: &'static str },
    DuplicateDependency { dependency: String },
    DependencyCycle { chain: Vec<String> },
    MissingArtifact { source: String },
    ArtifactMismatch { field: &'static str },
    OutcomeSourceMismatch,
    OutcomeDependencyMismatch,
    FailedImportWithoutDiagnostic,
    TransactionClosed,
    StaleTransaction { expected: u64, actual: u64 },
    RejectedTransaction { reason: String },
    ReadyArtifactLimit,
}

impl std::fmt::Display for JetGameAssetPipelineError {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyIdentity { field } => {
                write!(output, "game asset {field} must not be empty")
            }
            Self::IdentityTooLong { field } => {
                write!(output, "game asset {field} exceeds the Prelude identity limit")
            }
            Self::ControlIdentity { field } => {
                write!(output, "game asset {field} contains a control character")
            }
            Self::InvalidPath { path } => write!(output, "invalid game asset path `{path}`"),
            Self::PathEscape { path } => {
                write!(output, "game asset path escapes its declared root: `{path}`")
            }
            Self::UndeclaredPath { path } => {
                write!(output, "game asset path is not under a declared root: `{path}`")
            }
            Self::DuplicateRoot { value } => {
                write!(output, "game asset root `{value}` is declared more than once")
            }
            Self::RootNotDeclared { id } => {
                write!(output, "game asset root `{id}` is not declared")
            }
            Self::RootMismatch { expected, actual } => write!(
                output,
                "game asset root mismatch: expected `{expected}`, got `{actual}`"
            ),
            Self::TooManyRoots => write!(output, "game asset root limit exceeded"),
            Self::TooManyDependencies => write!(output, "game asset dependency limit exceeded"),
            Self::TooManyEvents => write!(output, "game asset watch event limit exceeded"),
            Self::TooManyDiagnostics => write!(output, "game asset diagnostic limit exceeded"),
            Self::TooManyOutputs => write!(output, "game asset output limit exceeded"),
            Self::TooManyTransactionItems => {
                write!(output, "game asset reload transaction limit exceeded")
            }
            Self::EmptyHash { field } => {
                write!(output, "game asset {field} hash must not be empty")
            }
            Self::DuplicateDependency { dependency } => {
                write!(output, "game asset dependency `{dependency}` is duplicated")
            }
            Self::DependencyCycle { chain } => {
                write!(output, "game asset dependency cycle: {}", chain.join(" -> "))
            }
            Self::MissingArtifact { source } => {
                write!(output, "game asset import returned no artifact for `{source}`")
            }
            Self::ArtifactMismatch { field } => {
                write!(output, "game asset artifact does not match plan {field}")
            }
            Self::OutcomeSourceMismatch => {
                write!(output, "game asset import outcome source does not match plan")
            }
            Self::OutcomeDependencyMismatch => {
                write!(output, "game asset import outcome dependencies do not match plan")
            }
            Self::FailedImportWithoutDiagnostic => {
                write!(output, "failed game asset import has no source-linked diagnostic")
            }
            Self::TransactionClosed => write!(output, "game asset reload transaction is closed"),
            Self::StaleTransaction { expected, actual } => write!(
                output,
                "stale game asset reload transaction: expected revision {expected}, current revision {actual}"
            ),
            Self::RejectedTransaction { reason } => {
                write!(output, "game asset reload transaction rejected: {reason}")
            }
            Self::ReadyArtifactLimit => write!(output, "ready game asset artifact limit exceeded"),
        }
    }
}

impl std::error::Error for JetGameAssetPipelineError {}

fn jet_game_asset_text(
    value: impl Into<String>,
    field: &'static str,
) -> Result<String, JetGameAssetPipelineError> {
    let value = value.into();
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(JetGameAssetPipelineError::EmptyIdentity { field });
    }
    if trimmed.len() > JET_GAME_ASSET_MAX_ID_BYTES {
        return Err(JetGameAssetPipelineError::IdentityTooLong { field });
    }
    if trimmed.chars().any(char::is_control) {
        return Err(JetGameAssetPipelineError::ControlIdentity { field });
    }
    Ok(trimmed.to_owned())
}
fn jet_game_asset_metadata(value: impl Into<String>) -> Result<String, JetGameAssetPipelineError> {
    let value = value.into();
    if value.len() > JET_GAME_ASSET_MAX_METADATA_BYTES {
        return Err(JetGameAssetPipelineError::IdentityTooLong {
            field: "runtime metadata",
        });
    }
    if value.chars().any(char::is_control) {
        return Err(JetGameAssetPipelineError::ControlIdentity {
            field: "runtime metadata",
        });
    }
    Ok(value)
}


fn jet_game_asset_path_text(value: impl Into<String>) -> Result<String, JetGameAssetPipelineError> {
    let value = value.into();
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(JetGameAssetPipelineError::InvalidPath { path: value });
    }
    if trimmed.len() > JET_GAME_ASSET_MAX_PATH_BYTES {
        return Err(JetGameAssetPipelineError::IdentityTooLong { field: "path" });
    }
    if trimmed.chars().any(char::is_control) {
        return Err(JetGameAssetPipelineError::ControlIdentity { field: "path" });
    }
    Ok(trimmed.replace('\\', "/"))
}

/// Normalize a physical root or event path without consulting the host.
/// `..` may remove an already-seen component, but may never walk above the
/// lexical beginning.  This is the path-escape boundary used by every caller.
fn jet_game_asset_normalize_path(
    value: impl Into<String>,
) -> Result<String, JetGameAssetPipelineError> {
    let original = value.into();
    let path = jet_game_asset_path_text(original.clone())?;
    let absolute_slash = path.starts_with('/');
    let drive_prefix = path.len() >= 3
        && path.as_bytes()[1] == b':'
        && path.as_bytes()[2] == b'/';
    let prefix_len = if absolute_slash {
        1
    } else if drive_prefix {
        3
    } else {
        0
    };
    let prefix = &path[..prefix_len];
    let mut components = Vec::<&str>::new();
    for component in path[prefix_len..].split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err(JetGameAssetPipelineError::PathEscape { path: original });
                }
            }
            component => {
                if component.chars().any(char::is_control) {
                    return Err(JetGameAssetPipelineError::ControlIdentity { field: "path" });
                }
                components.push(component);
            }
        }
    }
    if components.is_empty() {
        if absolute_slash || drive_prefix {
            return Ok(prefix.to_owned());
        }
        return Ok(".".to_owned());
    }
    let mut normalized = String::with_capacity(path.len());
    normalized.push_str(prefix);
    normalized.push_str(&components.join("/"));
    Ok(normalized)
}

fn jet_game_asset_normalize_logical_path(
    value: impl Into<String>,
) -> Result<String, JetGameAssetPipelineError> {
    let original = value.into();
    let path = jet_game_asset_path_text(original.clone())?;
    if path.starts_with('/')
        || (path.len() >= 3 && path.as_bytes()[1] == b':' && path.as_bytes()[2] == b'/')
    {
        return Err(JetGameAssetPipelineError::InvalidPath { path: original });
    }
    let mut components = Vec::<&str>::new();
    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err(JetGameAssetPipelineError::PathEscape { path: original });
                }
            }
            component => components.push(component),
        }
    }
    if components.is_empty() {
        return Err(JetGameAssetPipelineError::InvalidPath { path: original });
    }
    Ok(components.join("/"))
}

fn jet_game_asset_relative_path(root: &str, path: &str) -> Option<String> {
    if root == "." {
        if path.starts_with('/')
            || (path.len() >= 3 && path.as_bytes()[1] == b':' && path.as_bytes()[2] == b'/')
        {
            return None;
        }
        return (path != ".").then(|| path.to_owned());
    }
    if root == "/" {
        return (path.starts_with('/') && path.len() > 1).then(|| path[1..].to_owned());
    }
    let prefix = format!("{root}/");
    path.strip_prefix(&prefix)
        .filter(|relative| !relative.is_empty())
        .map(ToOwned::to_owned)
}

fn jet_game_asset_path_key(root: &str, logical_path: &str) -> String {
    format!("{}:{}:{logical_path}", root.len(), root)
}

fn jet_game_asset_json_string(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character.is_control() => {
                output.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => output.push(character),
        }
    }
    output.push('"');
    output
}

/// A declared physical root.  The `id` is the stable authored identity; the
/// normalized path is only a boundary fact and is never used to read the host.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct JetGameAssetRootIdentity {
    pub id: String,
    pub path: String,
}

impl JetGameAssetRootIdentity {
    pub fn new(
        id: impl Into<String>,
        path: impl Into<String>,
    ) -> Result<Self, JetGameAssetPipelineError> {
        let id = jet_game_asset_text(id, "root id")?;
        let path = jet_game_asset_normalize_path(path)?;
        Ok(Self { id, path })
    }

    pub fn validate(&self) -> Result<(), JetGameAssetPipelineError> {
        let expected = Self::new(self.id.clone(), self.path.clone())?;
        if *self == expected {
            Ok(())
        } else {
            Err(JetGameAssetPipelineError::InvalidPath {
                path: self.path.clone(),
            })
        }
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"id\":{},\"path\":{}}}",
            jet_game_asset_json_string(&self.id),
            jet_game_asset_json_string(&self.path),
        )
    }
}

/// The declared-root set is the only path authority in this kernel.  It does
/// not inspect whether roots exist and does not resolve symlinks.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JetGameAssetRootSet {
    roots: Vec<JetGameAssetRootIdentity>,
}

impl JetGameAssetRootSet {
    pub fn new(
        roots: Vec<JetGameAssetRootIdentity>,
    ) -> Result<Self, JetGameAssetPipelineError> {
        let mut set = Self::default();
        for root in roots {
            set.declare(root)?;
        }
        Ok(set)
    }

    pub fn declare(
        &mut self,
        root: JetGameAssetRootIdentity,
    ) -> Result<(), JetGameAssetPipelineError> {
        root.validate()?;
        if self.roots.len() >= JET_GAME_ASSET_MAX_ROOTS {
            return Err(JetGameAssetPipelineError::TooManyRoots);
        }
        if self
            .roots
            .iter()
            .any(|current| current.id == root.id || current.path == root.path)
        {
            return Err(JetGameAssetPipelineError::DuplicateRoot {
                value: root.id.clone(),
            });
        }
        self.roots.push(root);
        self.roots.sort();
        Ok(())
    }

    pub fn roots(&self) -> &[JetGameAssetRootIdentity] {
        &self.roots
    }

    pub fn root(&self, id: &str) -> Option<&JetGameAssetRootIdentity> {
        self.roots.iter().find(|root| root.id == id)
    }

    pub fn resolve(
        &self,
        path: impl Into<String>,
    ) -> Result<JetGameAssetResolvedPath, JetGameAssetPipelineError> {
        let original = path.into();
        let normalized = jet_game_asset_normalize_path(original.clone())?;
        let mut matches = self
            .roots
            .iter()
            .filter_map(|root| {
                jet_game_asset_relative_path(&root.path, &normalized).map(|logical_path| {
                    (
                        root.path.split('/').count(),
                        root.id.clone(),
                        root.clone(),
                        logical_path,
                    )
                })
            })
            .collect::<Vec<_>>();
        matches.sort_by(|left, right| {
            right
                .0
                .cmp(&left.0)
                .then_with(|| left.1.cmp(&right.1))
        });
        let Some((_, _, root, logical_path)) = matches.into_iter().next() else {
            return Err(JetGameAssetPipelineError::UndeclaredPath { path: original });
        };
        Ok(JetGameAssetResolvedPath { root, logical_path })
    }

    pub fn contains_root(&self, root: &JetGameAssetRootIdentity) -> bool {
        self.roots.iter().any(|declared| declared == root)
    }

    pub fn render_json(&self) -> String {
        let roots = self
            .roots
            .iter()
            .map(JetGameAssetRootIdentity::render_json)
            .collect::<Vec<_>>()
            .join(",");
        format!("[{}]", roots)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameAssetResolvedPath {
    pub root: JetGameAssetRootIdentity,
    pub logical_path: String,
}

impl JetGameAssetResolvedPath {
    pub fn node(&self) -> JetGameAssetNodeIdentity {
        JetGameAssetNodeIdentity {
            root_id: self.root.id.clone(),
            logical_path: self.logical_path.clone(),
        }
    }
}

/// Authored bytes under one declared root.  The content hash is an identity,
/// not a promise that this module has read the file.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct JetGameAssetSourceIdentity {
    pub root: JetGameAssetRootIdentity,
    pub logical_path: String,
    pub content_hash: String,
}

impl JetGameAssetSourceIdentity {
    pub fn new(
        root: JetGameAssetRootIdentity,
        logical_path: impl Into<String>,
        content_hash: impl Into<String>,
    ) -> Result<Self, JetGameAssetPipelineError> {
        root.validate()?;
        let logical_path = jet_game_asset_normalize_logical_path(logical_path)?;
        let content_hash = jet_game_asset_text(content_hash, "source hash")?;
        Ok(Self {
            root,
            logical_path,
            content_hash,
        })
    }

    pub fn from_bytes(
        root: JetGameAssetRootIdentity,
        logical_path: impl Into<String>,
        bytes: &[u8],
    ) -> Result<Self, JetGameAssetPipelineError> {
        Self::new(root, logical_path, jet_game_asset_hash_bytes(bytes))
    }

    pub fn node(&self) -> JetGameAssetNodeIdentity {
        JetGameAssetNodeIdentity {
            root_id: self.root.id.clone(),
            logical_path: self.logical_path.clone(),
        }
    }

    pub fn absolute_path(&self) -> String {
        if self.root.path == "." {
            self.logical_path.clone()
        } else if self.root.path == "/" {
            format!("/{}", self.logical_path)
        } else {
            format!("{}/{}", self.root.path, self.logical_path)
        }
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"root\":{},\"logical_path\":{},\"content_hash\":{}}}",
            self.root.render_json(),
            jet_game_asset_json_string(&self.logical_path),
            jet_game_asset_json_string(&self.content_hash),
        )
    }
}

/// A source node without bytes.  Graph edges use this identity so dependency
/// ordering never depends on an importer-specific string convention.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct JetGameAssetNodeIdentity {
    pub root_id: String,
    pub logical_path: String,
}

impl JetGameAssetNodeIdentity {
    pub fn new(
        root_id: impl Into<String>,
        logical_path: impl Into<String>,
    ) -> Result<Self, JetGameAssetPipelineError> {
        Ok(Self {
            root_id: jet_game_asset_text(root_id, "root id")?,
            logical_path: jet_game_asset_normalize_logical_path(logical_path)?,
        })
    }

    pub fn from_source(source: &JetGameAssetSourceIdentity) -> Self {
        source.node()
    }

    pub fn key(&self) -> String {
        jet_game_asset_path_key(&self.root_id, &self.logical_path)
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"root_id\":{},\"logical_path\":{}}}",
            jet_game_asset_json_string(&self.root_id),
            jet_game_asset_json_string(&self.logical_path),
        )
    }
}

/// Dependency edges carry a distinct type from source nodes even though both
/// are represented by a declared root and a normalized logical path.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct JetGameAssetDependencyIdentity {
    pub root_id: String,
    pub logical_path: String,
}

impl JetGameAssetDependencyIdentity {
    pub fn new(
        root_id: impl Into<String>,
        logical_path: impl Into<String>,
    ) -> Result<Self, JetGameAssetPipelineError> {
        Ok(Self {
            root_id: jet_game_asset_text(root_id, "dependency root id")?,
            logical_path: jet_game_asset_normalize_logical_path(logical_path)?,
        })
    }

    pub fn from_root(
        root: &JetGameAssetRootIdentity,
        logical_path: impl Into<String>,
    ) -> Result<Self, JetGameAssetPipelineError> {
        Self::new(root.id.clone(), logical_path)
    }

    pub fn node(&self) -> JetGameAssetNodeIdentity {
        JetGameAssetNodeIdentity {
            root_id: self.root_id.clone(),
            logical_path: self.logical_path.clone(),
        }
    }

    pub fn key(&self) -> String {
        jet_game_asset_path_key(&self.root_id, &self.logical_path)
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"root_id\":{},\"logical_path\":{}}}",
            jet_game_asset_json_string(&self.root_id),
            jet_game_asset_json_string(&self.logical_path),
        )
    }
}

/// Recipe identity includes the importer kind and its recipe revision.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct JetGameAssetRecipeIdentity {
    pub name: String,
    pub version: String,
}

impl JetGameAssetRecipeIdentity {
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
    ) -> Result<Self, JetGameAssetPipelineError> {
        Ok(Self {
            name: jet_game_asset_text(name, "recipe name")?,
            version: jet_game_asset_text(version, "recipe version")?,
        })
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"name\":{},\"version\":{}}}",
            jet_game_asset_json_string(&self.name),
            jet_game_asset_json_string(&self.version),
        )
    }
}

/// Tool identity is separate from recipe identity: changing the executable or
/// its version invalidates the cache even when recipe text is unchanged.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct JetGameAssetToolIdentity {
    pub name: String,
    pub version: String,
}

impl JetGameAssetToolIdentity {
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
    ) -> Result<Self, JetGameAssetPipelineError> {
        Ok(Self {
            name: jet_game_asset_text(name, "tool name")?,
            version: jet_game_asset_text(version, "tool version")?,
        })
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"name\":{},\"version\":{}}}",
            jet_game_asset_json_string(&self.name),
            jet_game_asset_json_string(&self.version),
        )
    }
}

/// Version facts are opaque on purpose.  Hosts may use semantic versions,
/// monotonically numbered source revisions, or a content generation label.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct JetGameAssetVersionIdentity {
    pub value: String,
}

impl JetGameAssetVersionIdentity {
    pub fn new(value: impl Into<String>) -> Result<Self, JetGameAssetPipelineError> {
        Ok(Self {
            value: jet_game_asset_text(value, "version")?,
        })
    }

    pub fn numeric(value: u64) -> Self {
        Self {
            value: value.to_string(),
        }
    }

    pub fn render_json(&self) -> String {
        jet_game_asset_json_string(&self.value)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetGameAssetWatchEventKind {
    Created,
    Changed,
    Deleted,
}

impl JetGameAssetWatchEventKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Changed => "changed",
            Self::Deleted => "deleted",
        }
    }
}

/// A host-supplied event.  Constructing an event is pure; it does not query
/// the path or infer whether a file exists.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct JetGameAssetWatchEvent {
    pub root: JetGameAssetRootIdentity,
    pub logical_path: String,
    pub kind: JetGameAssetWatchEventKind,
}

impl JetGameAssetWatchEvent {
    pub fn new(
        root: JetGameAssetRootIdentity,
        logical_path: impl Into<String>,
        kind: JetGameAssetWatchEventKind,
    ) -> Result<Self, JetGameAssetPipelineError> {
        root.validate()?;
        Ok(Self {
            root,
            logical_path: jet_game_asset_normalize_logical_path(logical_path)?,
            kind,
        })
    }

    pub fn from_path(
        roots: &JetGameAssetRootSet,
        path: impl Into<String>,
        kind: JetGameAssetWatchEventKind,
    ) -> Result<Self, JetGameAssetPipelineError> {
        let resolved = roots.resolve(path)?;
        Self::new(resolved.root, resolved.logical_path, kind)
    }

    pub fn node(&self) -> JetGameAssetNodeIdentity {
        JetGameAssetNodeIdentity {
            root_id: self.root.id.clone(),
            logical_path: self.logical_path.clone(),
        }
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"root\":{},\"logical_path\":{},\"kind\":{}}}",
            self.root.render_json(),
            jet_game_asset_json_string(&self.logical_path),
            jet_game_asset_json_string(self.kind.as_str()),
        )
    }
}

/// Coalesced watch facts are sorted by typed node identity.  Multiple host
/// notifications for one path become one net event, so import planning cannot
/// depend on OS delivery order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JetGameAssetWatchFacts {
    pub events: Vec<JetGameAssetWatchEvent>,
    pub created: Vec<JetGameAssetNodeIdentity>,
    pub changed: Vec<JetGameAssetNodeIdentity>,
    pub deleted: Vec<JetGameAssetNodeIdentity>,
    /// Number of host watcher changes omitted by its bounded scan/event
    /// budget. This is a count rather than synthetic path identities: the
    /// host did not provide trustworthy identities for omitted files.
    pub skipped_count: usize,
}

impl JetGameAssetWatchFacts {
    pub fn coalesce(
        roots: &JetGameAssetRootSet,
        events: &[JetGameAssetWatchEvent],
    ) -> Result<Self, JetGameAssetPipelineError> {
        Self::coalesce_with_skipped(roots, events, 0)
    }

    pub fn coalesce_with_skipped(
        roots: &JetGameAssetRootSet,
        events: &[JetGameAssetWatchEvent],
        skipped_count: usize,
    ) -> Result<Self, JetGameAssetPipelineError> {
        if events.len() > JET_GAME_ASSET_MAX_EVENTS
            || skipped_count > JET_GAME_ASSET_MAX_SKIPPED
        {
            return Err(JetGameAssetPipelineError::TooManyEvents);
        }
        let mut buckets = std::collections::BTreeMap::<
            (String, String),
            (JetGameAssetWatchEvent, JetGameAssetWatchEventKind),
        >::new();
        for event in events {
            let Some(declared_root) = roots.root(&event.root.id) else {
                return Err(JetGameAssetPipelineError::RootNotDeclared {
                    id: event.root.id.clone(),
                });
            };
            if declared_root != &event.root {
                return Err(JetGameAssetPipelineError::RootMismatch {
                    expected: declared_root.path.clone(),
                    actual: event.root.path.clone(),
                });
            }
            let normalized = JetGameAssetWatchEvent::new(
                event.root.clone(),
                event.logical_path.clone(),
                event.kind,
            )?;
            let key = (normalized.root.id.clone(), normalized.logical_path.clone());
            if let Some((_, first)) = buckets.get(&key) {
                let kind = jet_game_asset_net_event_kind(*first, normalized.kind);
                if let Some(kind) = kind {
                    if let Some((current, _)) = buckets.get_mut(&key) {
                        current.kind = kind;
                    }
                } else {
                    buckets.remove(&key);
                }
            } else {
                buckets.insert(key, (normalized.clone(), normalized.kind));
            }
        }
        let mut facts = Self {
            skipped_count,
            ..Self::default()
        };
        for (_, (event, _)) in buckets {
            match event.kind {
                JetGameAssetWatchEventKind::Created => facts.created.push(event.node()),
                JetGameAssetWatchEventKind::Changed => facts.changed.push(event.node()),
                JetGameAssetWatchEventKind::Deleted => facts.deleted.push(event.node()),
            }
            facts.events.push(event);
        }
        facts.events.sort();
        facts.created.sort();
        facts.changed.sort();
        facts.deleted.sort();
        Ok(facts)
    }

    pub fn imported_nodes(&self) -> Vec<JetGameAssetNodeIdentity> {
        let mut nodes = self.created.clone();
        nodes.extend(self.changed.clone());
        nodes.sort();
        nodes.dedup();
        nodes
    }

    pub fn render_json(&self) -> String {
        fn nodes(values: &[JetGameAssetNodeIdentity]) -> String {
            values
                .iter()
                .map(JetGameAssetNodeIdentity::render_json)
                .collect::<Vec<_>>()
                .join(",")
        }
        let events = self
            .events
            .iter()
            .map(JetGameAssetWatchEvent::render_json)
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"events\":[{}],\"created\":[{}],\"changed\":[{}],\"deleted\":[{}],\"skipped_count\":{}}}",
            events,
            nodes(&self.created),
            nodes(&self.changed),
            nodes(&self.deleted),
            self.skipped_count,
        )
    }
}

fn jet_game_asset_net_event_kind(
    first: JetGameAssetWatchEventKind,
    last: JetGameAssetWatchEventKind,
) -> Option<JetGameAssetWatchEventKind> {
    match (first, last) {
        // A create followed by a delete leaves no source transition to import.
        (JetGameAssetWatchEventKind::Created, JetGameAssetWatchEventKind::Deleted) => None,
        // A delete followed by a create is a replacement of an existing node.
        (JetGameAssetWatchEventKind::Deleted, JetGameAssetWatchEventKind::Created) => {
            Some(JetGameAssetWatchEventKind::Changed)
        }
        (JetGameAssetWatchEventKind::Created, _) => Some(JetGameAssetWatchEventKind::Created),
        (_, JetGameAssetWatchEventKind::Deleted) => Some(JetGameAssetWatchEventKind::Deleted),
        _ => Some(JetGameAssetWatchEventKind::Changed),
    }
}

/// Normalized dependency graph.  Adding an edge is transactional: a cycle or
/// undeclared root leaves the prior graph untouched.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JetGameAssetDependencyGraph {
    roots: JetGameAssetRootSet,
    sources: std::collections::BTreeMap<JetGameAssetNodeIdentity, JetGameAssetSourceIdentity>,
    edges: std::collections::BTreeMap<
        JetGameAssetNodeIdentity,
        Vec<JetGameAssetDependencyIdentity>,
    >,
}

impl JetGameAssetDependencyGraph {
    pub fn new(roots: JetGameAssetRootSet) -> Self {
        Self {
            roots,
            sources: std::collections::BTreeMap::new(),
            edges: std::collections::BTreeMap::new(),
        }
    }

    pub fn roots(&self) -> &JetGameAssetRootSet {
        &self.roots
    }

    pub fn add_source(
        &mut self,
        source: JetGameAssetSourceIdentity,
    ) -> Result<JetGameAssetNodeIdentity, JetGameAssetPipelineError> {
        self.validate_source(&source)?;
        let node = source.node();
        self.sources.insert(node.clone(), source);
        self.edges.entry(node.clone()).or_default();
        Ok(node)
    }

    pub fn set_dependencies(
        &mut self,
        source: &JetGameAssetSourceIdentity,
        dependencies: Vec<JetGameAssetDependencyIdentity>,
    ) -> Result<(), JetGameAssetPipelineError> {
        self.validate_source(source)?;
        let node = source.node();
        let mut normalized = self.normalize_dependencies(dependencies)?;
        normalized.sort();
        normalized.dedup();
        let mut candidate = self.edges.clone();
        candidate.insert(node.clone(), normalized);
        jet_game_asset_graph_order(&candidate)?;
        self.sources.insert(node.clone(), source.clone());
        self.edges = candidate;
        Ok(())
    }

    pub fn add_dependency(
        &mut self,
        source: &JetGameAssetSourceIdentity,
        dependency: JetGameAssetDependencyIdentity,
    ) -> Result<(), JetGameAssetPipelineError> {
        let mut dependencies = self.dependencies_for(&source.node());
        dependencies.push(dependency);
        self.set_dependencies(source, dependencies)
    }

    pub fn dependencies_for(
        &self,
        source: &JetGameAssetNodeIdentity,
    ) -> Vec<JetGameAssetDependencyIdentity> {
        self.edges.get(source).cloned().unwrap_or_default()
    }

    pub fn source(&self, node: &JetGameAssetNodeIdentity) -> Option<&JetGameAssetSourceIdentity> {
        self.sources.get(node)
    }

    pub fn nodes(&self) -> impl Iterator<Item = &JetGameAssetNodeIdentity> {
        self.edges.keys()
    }

    pub fn topological_order(
        &self,
    ) -> Result<Vec<JetGameAssetNodeIdentity>, JetGameAssetPipelineError> {
        jet_game_asset_graph_order(&self.edges)
    }

    pub fn dependents_of(
        &self,
        dependency: &JetGameAssetDependencyIdentity,
    ) -> Vec<JetGameAssetNodeIdentity> {
        let wanted = dependency.node();
        let mut pending = std::collections::BTreeSet::new();
        let mut visited = std::collections::BTreeSet::new();
        for (node, dependencies) in &self.edges {
            if dependencies.iter().any(|item| item.node() == wanted) {
                pending.insert(node.clone());
            }
        }
        while let Some(node) = pending.pop_first() {
            if !visited.insert(node.clone()) {
                continue;
            }
            for (candidate, dependencies) in &self.edges {
                if dependencies.iter().any(|item| item.node() == node) {
                    pending.insert(candidate.clone());
                }
            }
        }
        visited.into_iter().collect()
    }

    pub fn render_json(&self) -> String {
        let mut rows = Vec::new();
        for (node, dependencies) in &self.edges {
            let dependencies = dependencies
                .iter()
                .map(JetGameAssetDependencyIdentity::render_json)
                .collect::<Vec<_>>()
                .join(",");
            rows.push(format!(
                "{{\"node\":{},\"dependencies\":[{}]}}",
                node.render_json(),
                dependencies,
            ));
        }
        format!("[{}]", rows.join(","))
    }

    fn validate_source(
        &self,
        source: &JetGameAssetSourceIdentity,
    ) -> Result<(), JetGameAssetPipelineError> {
        if !self.roots.contains_root(&source.root) {
            return if self.roots.root(&source.root.id).is_some() {
                Err(JetGameAssetPipelineError::RootMismatch {
                    expected: self
                        .roots
                        .root(&source.root.id)
                        .map(|root| root.path.clone())
                        .unwrap_or_default(),
                    actual: source.root.path.clone(),
                })
            } else {
                Err(JetGameAssetPipelineError::RootNotDeclared {
                    id: source.root.id.clone(),
                })
            };
        }
        Ok(())
    }

    fn normalize_dependencies(
        &self,
        dependencies: Vec<JetGameAssetDependencyIdentity>,
    ) -> Result<Vec<JetGameAssetDependencyIdentity>, JetGameAssetPipelineError> {
        if dependencies.len() > JET_GAME_ASSET_MAX_DEPENDENCIES {
            return Err(JetGameAssetPipelineError::TooManyDependencies);
        }
        let mut normalized = Vec::with_capacity(dependencies.len());
        for dependency in dependencies {
            let Some(root) = self.roots.root(&dependency.root_id) else {
                return Err(JetGameAssetPipelineError::RootNotDeclared {
                    id: dependency.root_id,
                });
            };
            let logical_path = jet_game_asset_normalize_logical_path(dependency.logical_path)?;
            if root.id.is_empty() {
                return Err(JetGameAssetPipelineError::RootNotDeclared { id: root.id.clone() });
            }
            normalized.push(JetGameAssetDependencyIdentity {
                root_id: root.id.clone(),
                logical_path,
            });
        }
        Ok(normalized)
    }
}

fn jet_game_asset_graph_order(
    edges: &std::collections::BTreeMap<
        JetGameAssetNodeIdentity,
        Vec<JetGameAssetDependencyIdentity>,
    >,
) -> Result<Vec<JetGameAssetNodeIdentity>, JetGameAssetPipelineError> {
    let mut nodes = std::collections::BTreeSet::new();
    for (node, dependencies) in edges {
        nodes.insert(node.clone());
        nodes.extend(dependencies.iter().map(JetGameAssetDependencyIdentity::node));
    }
    let mut indegree = nodes
        .iter()
        .map(|node| (node.clone(), 0usize))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut reverse = std::collections::BTreeMap::<
        JetGameAssetNodeIdentity,
        std::collections::BTreeSet<JetGameAssetNodeIdentity>,
    >::new();
    for (node, dependencies) in edges {
        for dependency in dependencies {
            let dependency = dependency.node();
            *indegree.entry(node.clone()).or_default() += 1;
            reverse.entry(dependency).or_default().insert(node.clone());
        }
    }
    let mut ready = indegree
        .iter()
        .filter_map(|(node, count)| (*count == 0).then_some(node.clone()))
        .collect::<std::collections::BTreeSet<_>>();
    let mut order = Vec::with_capacity(nodes.len());
    while let Some(node) = ready.pop_first() {
        order.push(node.clone());
        if let Some(dependents) = reverse.get(&node) {
            for dependent in dependents {
                let count = indegree
                    .get_mut(dependent)
                    .expect("graph reverse edge has an indegree");
                *count -= 1;
                if *count == 0 {
                    ready.insert(dependent.clone());
                }
            }
        }
    }
    if order.len() == nodes.len() {
        return Ok(order);
    }
    let cycle_nodes = nodes
        .into_iter()
        .filter(|node| !order.contains(node))
        .map(|node| node.key())
        .collect::<Vec<_>>();
    Err(JetGameAssetPipelineError::DependencyCycle { chain: cycle_nodes })
}

/// A concrete importer recipe invocation.  Creating a plan records no output
/// and performs no importer work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameAssetImportPlan {
    pub source: JetGameAssetSourceIdentity,
    pub recipe: JetGameAssetRecipeIdentity,
    pub tool: JetGameAssetToolIdentity,
    pub version: JetGameAssetVersionIdentity,
    pub dependencies: Vec<JetGameAssetDependencyIdentity>,
    pub cache_key: String,
}

impl JetGameAssetImportPlan {
    pub fn new(
        source: JetGameAssetSourceIdentity,
        recipe: JetGameAssetRecipeIdentity,
        tool: JetGameAssetToolIdentity,
        version: JetGameAssetVersionIdentity,
    ) -> Result<Self, JetGameAssetPipelineError> {
        let mut plan = Self {
            source,
            recipe,
            tool,
            version,
            dependencies: Vec::new(),
            cache_key: String::new(),
        };
        plan.cache_key = plan.compute_cache_key();
        plan.validate()?;
        Ok(plan)
    }

    pub fn from_source(
        source: JetGameAssetSourceIdentity,
        recipe: JetGameAssetRecipeIdentity,
        tool: JetGameAssetToolIdentity,
        version: JetGameAssetVersionIdentity,
    ) -> Result<Self, JetGameAssetPipelineError> {
        Self::new(source, recipe, tool, version)
    }

    pub fn with_dependencies(
        mut self,
        dependencies: Vec<JetGameAssetDependencyIdentity>,
    ) -> Result<Self, JetGameAssetPipelineError> {
        self.dependencies = jet_game_asset_normalize_dependency_list(dependencies)?;
        self.cache_key = self.compute_cache_key();
        self.validate()?;
        Ok(self)
    }

    pub fn compute_cache_key(&self) -> String {
        jet_game_asset_import_cache_key(
            &self.source.content_hash,
            &self.recipe,
            &self.tool,
            &self.version,
            &self.dependencies,
        )
    }

    pub fn validate(&self) -> Result<(), JetGameAssetPipelineError> {
        self.source.root.validate()?;
        jet_game_asset_normalize_logical_path(self.source.logical_path.clone())?;
        if self.source.content_hash.trim().is_empty() {
            return Err(JetGameAssetPipelineError::EmptyHash { field: "source" });
        }
        if self.dependencies.len() > JET_GAME_ASSET_MAX_DEPENDENCIES {
            return Err(JetGameAssetPipelineError::TooManyDependencies);
        }
        if self.cache_key != self.compute_cache_key() {
            return Err(JetGameAssetPipelineError::ArtifactMismatch { field: "cache key" });
        }
        Ok(())
    }

    pub fn node(&self) -> JetGameAssetNodeIdentity {
        self.source.node()
    }

    pub fn render_json(&self) -> String {
        let dependencies = self
            .dependencies
            .iter()
            .map(JetGameAssetDependencyIdentity::render_json)
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"source\":{},\"recipe\":{},\"tool\":{},\"version\":{},\"dependencies\":[{}],\"cache_key\":{}}}",
            self.source.render_json(),
            self.recipe.render_json(),
            self.tool.render_json(),
            self.version.render_json(),
            dependencies,
            jet_game_asset_json_string(&self.cache_key),
        )
    }
}

fn jet_game_asset_normalize_dependency_list(
    dependencies: Vec<JetGameAssetDependencyIdentity>,
) -> Result<Vec<JetGameAssetDependencyIdentity>, JetGameAssetPipelineError> {
    if dependencies.len() > JET_GAME_ASSET_MAX_DEPENDENCIES {
        return Err(JetGameAssetPipelineError::TooManyDependencies);
    }
    let mut normalized = dependencies
        .into_iter()
        .map(|dependency| {
            Ok(JetGameAssetDependencyIdentity {
                root_id: jet_game_asset_text(dependency.root_id, "dependency root id")?,
                logical_path: jet_game_asset_normalize_logical_path(dependency.logical_path)?,
            })
        })
        .collect::<Result<Vec<_>, JetGameAssetPipelineError>>()?;
    normalized.sort();
    normalized.dedup();
    Ok(normalized)
}

fn jet_game_asset_cache_component(value: &str, output: &mut String) {
    output.push_str(&value.len().to_string());
    output.push(':');
    output.push_str(value);
    output.push(';');
}

fn jet_game_asset_import_cache_key(
    source_hash: &str,
    recipe: &JetGameAssetRecipeIdentity,
    tool: &JetGameAssetToolIdentity,
    version: &JetGameAssetVersionIdentity,
    dependencies: &[JetGameAssetDependencyIdentity],
) -> String {
    let mut material = String::from("jet-game-asset-cache-v1;");
    jet_game_asset_cache_component(source_hash, &mut material);
    jet_game_asset_cache_component(&recipe.name, &mut material);
    jet_game_asset_cache_component(&recipe.version, &mut material);
    jet_game_asset_cache_component(&tool.name, &mut material);
    jet_game_asset_cache_component(&tool.version, &mut material);
    jet_game_asset_cache_component(&version.value, &mut material);
    let mut dependencies = dependencies.to_vec();
    dependencies.sort();
    dependencies.dedup();
    for dependency in dependencies {
        jet_game_asset_cache_component(&dependency.root_id, &mut material);
        jet_game_asset_cache_component(&dependency.logical_path, &mut material);
    }
    jet_game_asset_hash_bytes(material.as_bytes())
}

/// Public free-function form for hosts that need a cache key before building a
/// full plan.  It hashes facts only; source path is provenance, not cache data.
pub fn jet_game_asset_cache_key(
    source_hash: &str,
    recipe: &JetGameAssetRecipeIdentity,
    tool: &JetGameAssetToolIdentity,
    version: &JetGameAssetVersionIdentity,
    dependencies: &[JetGameAssetDependencyIdentity],
) -> String {
    jet_game_asset_import_cache_key(source_hash, recipe, tool, version, dependencies)
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetGameAssetImportStatus {
    Ready,
    Failed,
}

impl JetGameAssetImportStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetGameAssetDiagnosticSeverity {
    Info,
    Warning,
    Error,
}

impl JetGameAssetDiagnosticSeverity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

/// Every error diagnostic points back to the source identity that produced it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameAssetImportDiagnostic {
    pub severity: JetGameAssetDiagnosticSeverity,
    pub code: String,
    pub message: String,
    pub source: JetGameAssetSourceIdentity,
    pub dependency: Option<JetGameAssetDependencyIdentity>,
}

impl JetGameAssetImportDiagnostic {
    pub fn new(
        severity: JetGameAssetDiagnosticSeverity,
        source: JetGameAssetSourceIdentity,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<Self, JetGameAssetPipelineError> {
        Ok(Self {
            severity,
            source,
            code: jet_game_asset_text(code, "diagnostic code")?,
            message: jet_game_asset_text(message, "diagnostic message")?,
            dependency: None,
        })
    }

    pub fn error(
        source: JetGameAssetSourceIdentity,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<Self, JetGameAssetPipelineError> {
        Self::new(JetGameAssetDiagnosticSeverity::Error, source, code, message)
    }

    pub fn with_dependency(
        mut self,
        dependency: JetGameAssetDependencyIdentity,
    ) -> Result<Self, JetGameAssetPipelineError> {
        self.dependency = Some(dependency);
        Ok(self)
    }

    pub fn render_json(&self) -> String {
        let dependency = self
            .dependency
            .as_ref()
            .map(JetGameAssetDependencyIdentity::render_json)
            .unwrap_or_else(|| "null".to_owned());
        format!(
            "{{\"severity\":{},\"code\":{},\"message\":{},\"source\":{},\"dependency\":{}}}",
            jet_game_asset_json_string(self.severity.as_str()),
            jet_game_asset_json_string(&self.code),
            jet_game_asset_json_string(&self.message),
            self.source.render_json(),
            dependency,
        )
    }
}

/// Output identity returned by a concrete importer.  The importer is outside
/// this module; this value is only checked against the plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameAssetArtifactIdentity {
    pub cache_key: String,
    pub source: JetGameAssetSourceIdentity,
    pub recipe: JetGameAssetRecipeIdentity,
    pub tool: JetGameAssetToolIdentity,
    pub version: JetGameAssetVersionIdentity,
    pub output_hash: String,
}

impl JetGameAssetArtifactIdentity {
    pub fn new(
        plan: &JetGameAssetImportPlan,
        output_hash: impl Into<String>,
    ) -> Result<Self, JetGameAssetPipelineError> {
        plan.validate()?;
        let output_hash = jet_game_asset_text(output_hash, "output hash")?;
        Ok(Self {
            cache_key: plan.cache_key.clone(),
            source: plan.source.clone(),
            recipe: plan.recipe.clone(),
            tool: plan.tool.clone(),
            version: plan.version.clone(),
            output_hash,
        })
    }

    pub fn compatible_with(
        &self,
        plan: &JetGameAssetImportPlan,
    ) -> Result<(), JetGameAssetPipelineError> {
        if self.cache_key != plan.cache_key {
            return Err(JetGameAssetPipelineError::ArtifactMismatch { field: "cache key" });
        }
        if self.source != plan.source {
            return Err(JetGameAssetPipelineError::ArtifactMismatch { field: "source" });
        }
        if self.recipe != plan.recipe {
            return Err(JetGameAssetPipelineError::ArtifactMismatch { field: "recipe" });
        }
        if self.tool != plan.tool {
            return Err(JetGameAssetPipelineError::ArtifactMismatch { field: "tool" });
        }
        if self.version != plan.version {
            return Err(JetGameAssetPipelineError::ArtifactMismatch { field: "version" });
        }
        Ok(())
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"cache_key\":{},\"source\":{},\"recipe\":{},\"tool\":{},\"version\":{},\"output_hash\":{}}}",
            jet_game_asset_json_string(&self.cache_key),
            self.source.render_json(),
            self.recipe.render_json(),
            self.tool.render_json(),
            self.version.render_json(),
            jet_game_asset_json_string(&self.output_hash),
        )
    }
}
/// Checked runtime payload handed to a game handle after import.  The
/// artifact identity is authoritative; bytes and metadata are opaque importer
/// output and never trigger filesystem reads or policy decisions here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameAssetRuntimeFact {
    pub artifact: JetGameAssetArtifactIdentity,
    pub bytes: Vec<u8>,
    pub metadata: String,
}

impl JetGameAssetRuntimeFact {
    pub fn new(
        artifact: JetGameAssetArtifactIdentity,
        bytes: Vec<u8>,
        metadata: impl Into<String>,
    ) -> Result<Self, JetGameAssetPipelineError> {
        Ok(Self {
            artifact,
            bytes,
            metadata: jet_game_asset_metadata(metadata)?,
        })
    }

    pub fn from_ready(
        plan: &JetGameAssetImportPlan,
        outcome: &JetGameAssetImportOutcome,
        bytes: Vec<u8>,
        metadata: impl Into<String>,
    ) -> Result<Self, JetGameAssetPipelineError> {
        outcome.validate_against(plan)?;
        let artifact = outcome
            .artifact()
            .cloned()
            .ok_or_else(|| JetGameAssetPipelineError::MissingArtifact {
                source: plan.source.logical_path.clone(),
            })?;
        Self::new(artifact, bytes, metadata)
    }

    pub fn validate_against(
        &self,
        plan: &JetGameAssetImportPlan,
        outcome: &JetGameAssetImportOutcome,
    ) -> Result<(), JetGameAssetPipelineError> {
        outcome.validate_against(plan)?;
        self.artifact.compatible_with(plan)?;
        jet_game_asset_metadata(self.metadata.clone())?;
        if outcome.artifact() != Some(&self.artifact) {
            return Err(JetGameAssetPipelineError::ArtifactMismatch {
                field: "runtime fact",
            });
        }
        Ok(())
    }

    pub fn byte_len(&self) -> u64 {
        self.bytes.len() as u64
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"artifact\":{},\"byte_len\":{},\"metadata\":{}}}",
            self.artifact.render_json(),
            self.byte_len(),
            jet_game_asset_json_string(&self.metadata),
        )
    }
}


/// Checked facts from an importer.  `Ready` must carry one checked artifact;
/// `Failed` must carry a source-linked diagnostic and never erases an artifact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameAssetImportOutcome {
    pub status: JetGameAssetImportStatus,
    pub source: JetGameAssetSourceIdentity,
    pub artifacts: Vec<JetGameAssetArtifactIdentity>,
    pub dependencies: Vec<JetGameAssetDependencyIdentity>,
    pub diagnostics: Vec<JetGameAssetImportDiagnostic>,
}

impl JetGameAssetImportOutcome {
    pub fn ready(
        plan: &JetGameAssetImportPlan,
        output_hash: impl Into<String>,
    ) -> Result<Self, JetGameAssetPipelineError> {
        let artifact = JetGameAssetArtifactIdentity::new(plan, output_hash)?;
        Ok(Self {
            status: JetGameAssetImportStatus::Ready,
            source: plan.source.clone(),
            artifacts: vec![artifact],
            dependencies: plan.dependencies.clone(),
            diagnostics: Vec::new(),
        })
    }

    pub fn failed(
        plan: &JetGameAssetImportPlan,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<Self, JetGameAssetPipelineError> {
        let diagnostic = JetGameAssetImportDiagnostic::error(
            plan.source.clone(),
            code,
            message,
        )?;
        Ok(Self {
            status: JetGameAssetImportStatus::Failed,
            source: plan.source.clone(),
            artifacts: Vec::new(),
            dependencies: plan.dependencies.clone(),
            diagnostics: vec![diagnostic],
        })
    }

    pub fn validate_against(
        &self,
        plan: &JetGameAssetImportPlan,
    ) -> Result<(), JetGameAssetPipelineError> {
        plan.validate()?;
        if self.source != plan.source {
            return Err(JetGameAssetPipelineError::OutcomeSourceMismatch);
        }
        let mut expected = plan.dependencies.clone();
        expected.sort();
        expected.dedup();
        let mut actual = self.dependencies.clone();
        actual.sort();
        actual.dedup();
        if expected != actual {
            return Err(JetGameAssetPipelineError::OutcomeDependencyMismatch);
        }
        if self.diagnostics.len() > JET_GAME_ASSET_MAX_DIAGNOSTICS {
            return Err(JetGameAssetPipelineError::TooManyDiagnostics);
        }
        if self.artifacts.len() > JET_GAME_ASSET_MAX_OUTPUTS {
            return Err(JetGameAssetPipelineError::TooManyOutputs);
        }
        match self.status {
            JetGameAssetImportStatus::Ready => {
                let Some(artifact) = self.artifacts.first() else {
                    return Err(JetGameAssetPipelineError::MissingArtifact {
                        source: self.source.logical_path.clone(),
                    });
                };
                for artifact in &self.artifacts {
                    artifact.compatible_with(plan)?;
                }
                if artifact.output_hash.trim().is_empty() {
                    return Err(JetGameAssetPipelineError::EmptyHash { field: "output" });
                }
            }
            JetGameAssetImportStatus::Failed => {
                if !self.artifacts.is_empty() {
                    return Err(JetGameAssetPipelineError::ArtifactMismatch {
                        field: "failed outcome output",
                    });
                }
                if !self.diagnostics.iter().any(|diagnostic| {
                    diagnostic.source == self.source
                        && diagnostic.severity == JetGameAssetDiagnosticSeverity::Error
                }) {
                    return Err(JetGameAssetPipelineError::FailedImportWithoutDiagnostic);
                }
            }
        }
        Ok(())
    }

    pub fn artifact(&self) -> Option<&JetGameAssetArtifactIdentity> {
        self.artifacts.first()
    }

    pub fn is_ready(&self) -> bool {
        self.status == JetGameAssetImportStatus::Ready
    }

    pub fn render_json(&self) -> String {
        let artifacts = self
            .artifacts
            .iter()
            .map(JetGameAssetArtifactIdentity::render_json)
            .collect::<Vec<_>>()
            .join(",");
        let dependencies = self
            .dependencies
            .iter()
            .map(JetGameAssetDependencyIdentity::render_json)
            .collect::<Vec<_>>()
            .join(",");
        let diagnostics = self
            .diagnostics
            .iter()
            .map(JetGameAssetImportDiagnostic::render_json)
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"status\":{},\"source\":{},\"artifacts\":[{}],\"dependencies\":[{}],\"diagnostics\":[{}]}}",
            jet_game_asset_json_string(self.status.as_str()),
            self.source.render_json(),
            artifacts,
            dependencies,
            diagnostics,
        )
    }
}

/// The import report contains only watch/import facts.  It intentionally has
/// no bytes and no importer callback, so the graph cannot grow a hidden scan.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JetGameAssetImportReport {
    pub imported: Vec<JetGameAssetNodeIdentity>,
    pub dependencies: Vec<JetGameAssetDependencyIdentity>,
    pub created: Vec<JetGameAssetNodeIdentity>,
    pub deleted: Vec<JetGameAssetNodeIdentity>,
    pub skipped: Vec<JetGameAssetNodeIdentity>,
    pub skipped_count: usize,
    pub diagnostics: Vec<JetGameAssetImportDiagnostic>,
}

impl JetGameAssetImportReport {
    pub fn from_watch(facts: &JetGameAssetWatchFacts) -> Self {
        let mut report = Self {
            imported: facts.imported_nodes(),
            dependencies: Vec::new(),
            created: facts.created.clone(),
            deleted: facts.deleted.clone(),
            skipped: Vec::new(),
            skipped_count: facts.skipped_count,
            diagnostics: Vec::new(),
        };
        report.sort_and_dedup();
        report
    }

    pub fn record_dependency(&mut self, dependency: JetGameAssetDependencyIdentity) {
        if self.dependencies.len() < JET_GAME_ASSET_MAX_DEPENDENCIES {
            self.dependencies.push(dependency);
            self.dependencies.sort();
            self.dependencies.dedup();
        }
    }

    pub fn record_skipped(&mut self, node: JetGameAssetNodeIdentity) {
        if self.skipped.len() < JET_GAME_ASSET_MAX_EVENTS {
            self.skipped.push(node);
            self.skipped.sort();
            self.skipped.dedup();
        }
    }

    pub fn record_diagnostic(
        &mut self,
        diagnostic: JetGameAssetImportDiagnostic,
    ) -> Result<(), JetGameAssetPipelineError> {
        if self.diagnostics.len() >= JET_GAME_ASSET_MAX_DIAGNOSTICS {
            return Err(JetGameAssetPipelineError::TooManyDiagnostics);
        }
        self.diagnostics.push(diagnostic);
        self.diagnostics.sort_by(|left, right| {
            left.source
                .node()
                .cmp(&right.source.node())
                .then_with(|| left.code.cmp(&right.code))
        });
        Ok(())
    }

    pub fn sort_and_dedup(&mut self) {
        for values in [
            &mut self.imported,
            &mut self.created,
            &mut self.deleted,
            &mut self.skipped,
        ] {
            values.sort();
            values.dedup();
        }
        self.dependencies.sort();
        self.dependencies.dedup();
    }

    pub fn render_json(&self) -> String {
        fn nodes(values: &[JetGameAssetNodeIdentity]) -> String {
            values
                .iter()
                .map(JetGameAssetNodeIdentity::render_json)
                .collect::<Vec<_>>()
                .join(",")
        }
        let dependencies = self
            .dependencies
            .iter()
            .map(JetGameAssetDependencyIdentity::render_json)
            .collect::<Vec<_>>()
            .join(",");
        let diagnostics = self
            .diagnostics
            .iter()
            .map(JetGameAssetImportDiagnostic::render_json)
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"imported\":[{}],\"dependencies\":[{}],\"created\":[{}],\"deleted\":[{}],\"skipped\":[{}],\"skipped_count\":{},\"diagnostics\":[{}]}}",
            nodes(&self.imported),
            dependencies,
            nodes(&self.created),
            nodes(&self.deleted),
            nodes(&self.skipped),
            self.skipped_count,
            diagnostics,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetGameAssetReloadStatus {
    Applied,
    Unchanged,
    Failed,
    RolledBack,
    Rejected,
}

impl JetGameAssetReloadStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::Unchanged => "unchanged",
            Self::Failed => "failed",
            Self::RolledBack => "rolled_back",
            Self::Rejected => "rejected",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetGameAssetTransactionStatus {
    Committed,
    RolledBack,
    Rejected,
}

impl JetGameAssetTransactionStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Committed => "committed",
            Self::RolledBack => "rolled_back",
            Self::Rejected => "rejected",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameAssetReloadReceipt {
    pub transaction_id: u64,
    pub source: JetGameAssetSourceIdentity,
    pub status: JetGameAssetReloadStatus,
    pub artifact: Option<JetGameAssetArtifactIdentity>,
    pub diagnostics: Vec<JetGameAssetImportDiagnostic>,
    pub preserved: bool,
    pub revision_before: u64,
    pub revision_after: u64,
}

impl JetGameAssetReloadReceipt {
    pub fn render_json(&self) -> String {
        let artifact = self
            .artifact
            .as_ref()
            .map(JetGameAssetArtifactIdentity::render_json)
            .unwrap_or_else(|| "null".to_owned());
        let diagnostics = self
            .diagnostics
            .iter()
            .map(JetGameAssetImportDiagnostic::render_json)
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"transaction_id\":{},\"source\":{},\"status\":{},\"artifact\":{},\"diagnostics\":[{}],\"preserved\":{},\"revision_before\":{},\"revision_after\":{}}}",
            self.transaction_id,
            self.source.render_json(),
            jet_game_asset_json_string(self.status.as_str()),
            artifact,
            diagnostics,
            self.preserved,
            self.revision_before,
            self.revision_after,
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetGameAssetTransactionReceipt {
    pub transaction_id: u64,
    pub status: JetGameAssetTransactionStatus,
    pub revision_before: u64,
    pub revision_after: u64,
    pub entries: Vec<JetGameAssetReloadReceipt>,
    pub reason: Option<String>,
}

impl JetGameAssetTransactionReceipt {
    pub fn is_committed(&self) -> bool {
        self.status == JetGameAssetTransactionStatus::Committed
    }

    pub fn render_json(&self) -> String {
        let entries = self
            .entries
            .iter()
            .map(JetGameAssetReloadReceipt::render_json)
            .collect::<Vec<_>>()
            .join(",");
        let reason = self
            .reason
            .as_deref()
            .map(jet_game_asset_json_string)
            .unwrap_or_else(|| "null".to_owned());
        format!(
            "{{\"transaction_id\":{},\"status\":{},\"revision_before\":{},\"revision_after\":{},\"entries\":[{}],\"reason\":{}}}",
            self.transaction_id,
            jet_game_asset_json_string(self.status.as_str()),
            self.revision_before,
            self.revision_after,
            entries,
            reason,
        )
    }
}

#[derive(Debug)]
struct JetGameAssetStoreState {
    revision: u64,
    next_transaction_id: u64,
    ready: std::collections::BTreeMap<JetGameAssetNodeIdentity, JetGameAssetArtifactIdentity>,
    latest: std::collections::BTreeMap<JetGameAssetNodeIdentity, JetGameAssetReloadReceipt>,
}

/// Thread-safe in-memory replacement store.  It is a cache of checked facts,
/// not a filesystem watcher and not an importer registry.
#[derive(Clone, Debug)]
pub struct JetGameAssetStore {
    state: std::sync::Arc<std::sync::RwLock<JetGameAssetStoreState>>,
    roots: Option<JetGameAssetRootSet>,
    max_ready: usize,
}

impl Default for JetGameAssetStore {
    fn default() -> Self {
        Self::new()
    }
}

impl JetGameAssetStore {
    pub fn new() -> Self {
        Self::with_capacity(JET_GAME_ASSET_MAX_READY_ARTIFACTS)
    }

    pub fn with_capacity(max_ready: usize) -> Self {
        Self {
            state: std::sync::Arc::new(std::sync::RwLock::new(JetGameAssetStoreState {
                revision: 0,
                next_transaction_id: 1,
                ready: std::collections::BTreeMap::new(),
                latest: std::collections::BTreeMap::new(),
            })),
            roots: None,
            max_ready: max_ready.max(1).min(JET_GAME_ASSET_MAX_READY_ARTIFACTS),
        }
    }

    pub fn with_roots(
        roots: JetGameAssetRootSet,
    ) -> Result<Self, JetGameAssetPipelineError> {
        let mut store = Self::new();
        for root in roots.roots() {
            root.validate()?;
        }
        store.roots = Some(roots);
        Ok(store)
    }

    pub fn revision(&self) -> u64 {
        self.read_state().revision
    }

    pub fn ready(
        &self,
        source: &JetGameAssetSourceIdentity,
    ) -> Option<JetGameAssetArtifactIdentity> {
        self.read_state().ready.get(&source.node()).cloned()
    }

    pub fn ready_for_node(
        &self,
        node: &JetGameAssetNodeIdentity,
    ) -> Option<JetGameAssetArtifactIdentity> {
        self.read_state().ready.get(node).cloned()
    }

    pub fn ready_assets(&self) -> Vec<JetGameAssetArtifactIdentity> {
        self.read_state().ready.values().cloned().collect()
    }

    pub fn latest(
        &self,
        source: &JetGameAssetSourceIdentity,
    ) -> Option<JetGameAssetReloadReceipt> {
        self.read_state().latest.get(&source.node()).cloned()
    }

    pub fn begin_transaction(&self) -> JetGameAssetReloadTransaction {
        self.begin_at(self.revision())
    }

    pub fn begin_at(&self, expected_revision: u64) -> JetGameAssetReloadTransaction {
        let transaction_id = {
            let mut state = self.write_state();
            let id = state.next_transaction_id;
            state.next_transaction_id = state.next_transaction_id.saturating_add(1);
            id
        };
        JetGameAssetReloadTransaction {
            state: self.state.clone(),
            roots: self.roots.clone(),
            max_ready: self.max_ready,
            transaction_id,
            base_revision: expected_revision,
            staged: std::collections::BTreeMap::new(),
            closed: false,
        }
    }

    /// Convenience path for one checked import.  Structural rejection still
    /// returns an auditable receipt and never changes the ready map.
    pub fn apply(
        &self,
        plan: JetGameAssetImportPlan,
        outcome: JetGameAssetImportOutcome,
    ) -> JetGameAssetTransactionReceipt {
        let source = plan.source.clone();
        let mut transaction = self.begin_transaction();
        match transaction.stage(plan, outcome) {
            Ok(()) => transaction.commit(),
            Err(error) => transaction.reject(source, error),
        }
    }

    pub fn apply_at(
        &self,
        expected_revision: u64,
        plan: JetGameAssetImportPlan,
        outcome: JetGameAssetImportOutcome,
    ) -> JetGameAssetTransactionReceipt {
        let source = plan.source.clone();
        let mut transaction = self.begin_at(expected_revision);
        match transaction.stage(plan, outcome) {
            Ok(()) => transaction.commit(),
            Err(error) => transaction.reject(source, error),
        }
    }

    fn read_state(&self) -> std::sync::RwLockReadGuard<'_, JetGameAssetStoreState> {
        self.state
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn write_state(&self) -> std::sync::RwLockWriteGuard<'_, JetGameAssetStoreState> {
        self.state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// A reload transaction stages all checked outcomes first.  Commit validates
/// the revision and then publishes every successful replacement under one lock;
/// a failed importer entry preserves the prior artifact.
#[derive(Debug)]
pub struct JetGameAssetReloadTransaction {
    state: std::sync::Arc<std::sync::RwLock<JetGameAssetStoreState>>,
    roots: Option<JetGameAssetRootSet>,
    max_ready: usize,
    transaction_id: u64,
    base_revision: u64,
    staged: std::collections::BTreeMap<JetGameAssetNodeIdentity, (JetGameAssetImportPlan, JetGameAssetImportOutcome)>,
    closed: bool,
}

impl JetGameAssetReloadTransaction {
    pub fn id(&self) -> u64 {
        self.transaction_id
    }

    pub fn base_revision(&self) -> u64 {
        self.base_revision
    }

    pub fn staged_count(&self) -> usize {
        self.staged.len()
    }

    pub fn stage(
        &mut self,
        plan: JetGameAssetImportPlan,
        outcome: JetGameAssetImportOutcome,
    ) -> Result<(), JetGameAssetPipelineError> {
        if self.closed {
            return Err(JetGameAssetPipelineError::TransactionClosed);
        }
        if self.staged.len() >= JET_GAME_ASSET_MAX_TRANSACTION_ITEMS {
            return Err(JetGameAssetPipelineError::TooManyTransactionItems);
        }
        plan.validate()?;
        outcome.validate_against(&plan)?;
        if let Some(roots) = &self.roots {
            if !roots.contains_root(&plan.source.root) {
                return Err(JetGameAssetPipelineError::RootNotDeclared {
                    id: plan.source.root.id.clone(),
                });
            }
            for dependency in &plan.dependencies {
                if roots.root(&dependency.root_id).is_none() {
                    return Err(JetGameAssetPipelineError::RootNotDeclared {
                        id: dependency.root_id.clone(),
                    });
                }
            }
        }
        let node = plan.node();
        if self.staged.contains_key(&node) {
            return Err(JetGameAssetPipelineError::DuplicateDependency {
                dependency: node.key(),
            });
        }
        self.staged.insert(node, (plan, outcome));
        Ok(())
    }

    pub fn rollback(mut self) -> JetGameAssetTransactionReceipt {
        self.closed = true;
        let state = self
            .state
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let revision = state.revision;
        let entries = self
            .staged
            .values()
            .map(|(plan, outcome)| JetGameAssetReloadReceipt {
                transaction_id: self.transaction_id,
                source: plan.source.clone(),
                status: JetGameAssetReloadStatus::RolledBack,
                artifact: state.ready.get(&plan.node()).cloned(),
                diagnostics: outcome.diagnostics.clone(),
                preserved: state.ready.contains_key(&plan.node()),
                revision_before: revision,
                revision_after: revision,
            })
            .collect();
        JetGameAssetTransactionReceipt {
            transaction_id: self.transaction_id,
            status: JetGameAssetTransactionStatus::RolledBack,
            revision_before: revision,
            revision_after: revision,
            entries,
            reason: None,
        }
    }

    pub fn commit(mut self) -> JetGameAssetTransactionReceipt {
        self.closed = true;
        let mut state = self
            .state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let revision_before = state.revision;
        if revision_before != self.base_revision {
            let error = JetGameAssetPipelineError::StaleTransaction {
                expected: self.base_revision,
                actual: revision_before,
            };
            return self.rejected_locked(&state, error);
        }

        let mut replacements = Vec::<(
            JetGameAssetNodeIdentity,
            JetGameAssetArtifactIdentity,
        )>::new();
        for (plan, outcome) in self.staged.values() {
            if outcome.status == JetGameAssetImportStatus::Ready {
                let Some(artifact) = outcome.artifact() else {
                    return self.rejected_locked(
                        &state,
                        JetGameAssetPipelineError::MissingArtifact {
                            source: plan.source.logical_path.clone(),
                        },
                    );
                };
                if let Err(error) = artifact.compatible_with(plan) {
                    return self.rejected_locked(&state, error);
                }
                replacements.push((plan.node(), artifact.clone()));
            }
        }
        let new_nodes = replacements
            .iter()
            .filter(|(node, _)| !state.ready.contains_key(node))
            .count();
        if state.ready.len().saturating_add(new_nodes) > self.max_ready {
            return self.rejected_locked(&state, JetGameAssetPipelineError::ReadyArtifactLimit);
        }

        let previous = self
            .staged
            .keys()
            .map(|node| (node.clone(), state.ready.get(node).cloned()))
            .collect::<std::collections::BTreeMap<_, _>>();
        let changed = replacements.iter().any(|(node, artifact)| {
            previous.get(node).and_then(|value| value.as_ref()) != Some(artifact)
        });
        let revision_after = if changed {
            revision_before.saturating_add(1)
        } else {
            revision_before
        };
        state.revision = revision_after;
        for (node, artifact) in replacements {
            state.ready.insert(node, artifact);
        }
        let mut entries = Vec::with_capacity(self.staged.len());
        for (plan, outcome) in self.staged.values() {
            let node = plan.node();
            let prior = previous.get(&node).cloned().flatten();
            let (status, artifact, preserved) = match outcome.status {
                JetGameAssetImportStatus::Ready => {
                    let artifact = outcome.artifact().cloned();
                    let status = if prior.as_ref() == artifact.as_ref() {
                        JetGameAssetReloadStatus::Unchanged
                    } else {
                        JetGameAssetReloadStatus::Applied
                    };
                    (status, artifact, false)
                }
                JetGameAssetImportStatus::Failed => {
                    let preserved = prior.is_some();
                    (
                        JetGameAssetReloadStatus::Failed,
                        prior,
                        preserved,
                    )
                }
            };
            let receipt = JetGameAssetReloadReceipt {
                transaction_id: self.transaction_id,
                source: plan.source.clone(),
                status,
                artifact,
                diagnostics: outcome.diagnostics.clone(),
                preserved,
                revision_before,
                revision_after,
            };
            state.latest.insert(node, receipt.clone());
            entries.push(receipt);
        }
        JetGameAssetTransactionReceipt {
            transaction_id: self.transaction_id,
            status: JetGameAssetTransactionStatus::Committed,
            revision_before,
            revision_after,
            entries,
            reason: None,
        }
    }

    pub fn try_commit(
        self,
    ) -> Result<JetGameAssetTransactionReceipt, JetGameAssetPipelineError> {
        let receipt = self.commit();
        if receipt.status == JetGameAssetTransactionStatus::Rejected {
            let reason = receipt
                .reason
                .clone()
                .unwrap_or_else(|| "asset reload transaction rejected".to_owned());
            return Err(JetGameAssetPipelineError::RejectedTransaction { reason });
        }
        Ok(receipt)
    }

    fn reject(
        mut self,
        source: JetGameAssetSourceIdentity,
        error: JetGameAssetPipelineError,
    ) -> JetGameAssetTransactionReceipt {
        self.closed = true;
        let node = source.node();
        let state = self
            .state
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let revision = state.revision;
        let diagnostic = JetGameAssetImportDiagnostic::error(
            source.clone(),
            "E-ASSET-RELOAD",
            error.to_string(),
        )
        .ok();
        let entry = JetGameAssetReloadReceipt {
            transaction_id: self.transaction_id,
            source,
            status: JetGameAssetReloadStatus::Rejected,
            artifact: state.ready.get(&node).cloned(),
            diagnostics: diagnostic.into_iter().collect(),
            preserved: state.ready.contains_key(&node),
            revision_before: revision,
            revision_after: revision,
        };
        JetGameAssetTransactionReceipt {
            transaction_id: self.transaction_id,
            status: JetGameAssetTransactionStatus::Rejected,
            revision_before: revision,
            revision_after: revision,
            entries: vec![entry],
            reason: Some(error.to_string()),
        }
    }

    fn rejected_locked(
        &self,
        state: &JetGameAssetStoreState,
        error: JetGameAssetPipelineError,
    ) -> JetGameAssetTransactionReceipt {
        let revision = state.revision;
        let entries = self
            .staged
            .values()
            .map(|(plan, outcome)| JetGameAssetReloadReceipt {
                transaction_id: self.transaction_id,
                source: plan.source.clone(),
                status: JetGameAssetReloadStatus::Rejected,
                artifact: state.ready.get(&plan.node()).cloned(),
                diagnostics: {
                    let mut diagnostics = outcome.diagnostics.clone();
                    if let Ok(diagnostic) = JetGameAssetImportDiagnostic::error(
                        plan.source.clone(),
                        "E-ASSET-RELOAD",
                        error.to_string(),
                    ) {
                        diagnostics.push(diagnostic);
                    }
                    diagnostics
                },
                preserved: state.ready.contains_key(&plan.node()),
                revision_before: revision,
                revision_after: revision,
            })
            .collect();
        JetGameAssetTransactionReceipt {
            transaction_id: self.transaction_id,
            status: JetGameAssetTransactionStatus::Rejected,
            revision_before: self.base_revision,
            revision_after: revision,
            entries,
            reason: Some(error.to_string()),
        }
    }
}

fn jet_game_asset_hash_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = jet_game_asset_sha256(bytes);
    let mut output = String::with_capacity(71);
    output.push_str("sha256-");
    for byte in digest {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

// Small std-only SHA-256 used for source and cache identities.  Keeping the
// helper local makes this Core part usable before the host assembler wires the
// broader CoreLib SHA primitive; it has no filesystem or platform dependency.
fn jet_game_asset_sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1,
        0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
        0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
        0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
        0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
        0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut state = [
        0x6a09e667u32,
        0xbb67ae85,
        0x3c6ef372,
        0xa54ff53a,
        0x510e527f,
        0x9b05688c,
        0x1f83d9ab,
        0x5be0cd19,
    ];
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let padded_len = ((data.len() + 9).saturating_add(63) / 64) * 64;
    let mut padded = vec![0u8; padded_len];
    padded[..data.len()].copy_from_slice(data);
    padded[data.len()] = 0x80;
    padded[padded_len - 8..].copy_from_slice(&bit_len.to_be_bytes());
    for chunk in padded.chunks_exact(64) {
        let mut schedule = [0u32; 64];
        for (index, word) in schedule[..16].iter_mut().enumerate() {
            let base = index * 4;
            *word = u32::from_be_bytes([
                chunk[base],
                chunk[base + 1],
                chunk[base + 2],
                chunk[base + 3],
            ]);
        }
        for index in 16..64 {
            let x = schedule[index - 15];
            let y = schedule[index - 2];
            let sigma0 = x.rotate_right(7) ^ x.rotate_right(18) ^ (x >> 3);
            let sigma1 = y.rotate_right(17) ^ y.rotate_right(19) ^ (y >> 10);
            schedule[index] = schedule[index - 16]
                .wrapping_add(sigma0)
                .wrapping_add(schedule[index - 7])
                .wrapping_add(sigma1);
        }
        let mut working = state;
        for index in 0..64 {
            let a = working[0];
            let b = working[1];
            let c = working[2];
            let d = working[3];
            let e = working[4];
            let f = working[5];
            let g = working[6];
            let h = working[7];
            let sigma1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choice = (e & f) ^ ((!e) & g);
            let temporary1 = h
                .wrapping_add(sigma1)
                .wrapping_add(choice)
                .wrapping_add(K[index])
                .wrapping_add(schedule[index]);
            let sigma0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temporary2 = sigma0.wrapping_add(majority);
            working[7] = g;
            working[6] = f;
            working[5] = e;
            working[4] = d.wrapping_add(temporary1);
            working[3] = c;
            working[2] = b;
            working[1] = a;
            working[0] = temporary1.wrapping_add(temporary2);
        }
        for index in 0..8 {
            state[index] = state[index].wrapping_add(working[index]);
        }
    }
    let mut digest = [0u8; 32];
    for (index, word) in state.iter().copied().enumerate() {
        digest[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    digest
}
