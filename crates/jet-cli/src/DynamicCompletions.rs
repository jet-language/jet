//! Host-neutral dynamic completion facts and query engine.
//!
//! The engine consumes a checked [`CLICommandSchema`], explicit typed value
//! facts, and explicit filesystem roots. It never invokes a command, provider,
//! shell, network, or ambient value source. Callers select the command/input
//! fact before querying; this module only performs the bounded projection.

use jet_foundation::CLISchema::{CLICommandSchema, CLIInputSchema, CLIValueKind};
use jet_foundation::Diagnostics::Span;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// Maximum source text accepted by one completion request.
pub const MAX_COMPLETION_SOURCE_BYTES: usize = 1024 * 1024;
/// Maximum typed facts inspected by one query.
pub const MAX_TYPED_FACTS: usize = 4096;
/// Maximum values retained from one typed fact.
pub const MAX_FACT_VALUES: usize = 4096;
/// Maximum candidates returned by one query, regardless of caller limits.
pub const MAX_COMPLETION_CANDIDATES: usize = 256;
/// Maximum filesystem entries retained for one directory projection.
pub const MAX_DIRECTORY_ENTRIES: usize = 4096;
/// Maximum path components inspected by one filesystem projection.
pub const MAX_PATH_DEPTH: usize = 64;
/// Maximum explicitly granted roots.
pub const MAX_AUTHORITY_ROOTS: usize = 64;

/// The semantic family of a completion candidate.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CompletionKind {
    /// A declared enum variant.
    Enum,
    /// A declared tag value.
    Tag,
    /// A checked literal value.
    Literal,
    /// The two values of a checked boolean input.
    Bool,
    /// A filesystem entry from an explicitly granted root.
    Path,
    /// A checked, non-internal `#Job` name.
    Job,
}

impl CompletionKind {
    /// Stable host-neutral wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Enum => "enum",
            Self::Tag => "tag",
            Self::Literal => "literal",
            Self::Bool => "bool",
            Self::Path => "path",
            Self::Job => "job",
        }
    }
}

/// Freshness attached to a candidate and result.
///
/// Queries reject stale input before constructing candidates. `Stale` is kept
/// in the fact shape so a host can represent a cached result without inventing
/// another candidate schema; this engine emits only `Fresh` candidates.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CompletionFreshness {
    /// The candidate was built from the revisions supplied by the request.
    Fresh,
    /// The candidate came from a revision that no longer matches the request.
    Stale,
}

impl CompletionFreshness {
    /// Stable host-neutral wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Stale => "stale",
        }
    }
}

/// Which checked surface the request is asking to complete.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CompletionTarget {
    /// A value belonging to a checked schema input.
    Value,
    /// A path belonging to a checked `Path` input.
    Path,
    /// A visible checked job name.
    Jobs,
}

/// A replacement edit in source-byte coordinates.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionReplacement {
    /// Half-open source range to replace.
    pub span: Span,
    /// Text to place over `span`.
    pub text: String,
}

impl CompletionReplacement {
    /// Construct a replacement edit.
    pub fn new(span: Span, text: impl Into<String>) -> Self {
        Self {
            span,
            text: text.into(),
        }
    }
}

/// One host-neutral completion candidate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionCandidate {
    /// Display and filtering value.
    pub label: String,
    /// Semantic candidate family.
    pub kind: CompletionKind,
    /// Checked explanation suitable for an editor or shell UI.
    pub detail: String,
    /// Exact source edit for this candidate.
    pub replacement: CompletionReplacement,
    /// Stable origin label (`typed-schema`, `filesystem`, or `job-schema`).
    pub source: String,
    /// Revision state of the candidate.
    pub freshness: CompletionFreshness,
    /// Bounded deterministic rank; lower values sort first.
    pub rank: u32,
}

/// The result of one dynamic completion query.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionResult {
    /// Complete lexical token containing the cursor.
    pub token_span: Span,
    /// Portion of the value token before the cursor used for filtering.
    pub prefix: String,
    /// Exact value range replaced by every returned candidate.
    pub replacement_span: Span,
    /// Candidates in deterministic rank order.
    pub candidates: Vec<CompletionCandidate>,
    /// True when one or more bounded inputs or outputs were omitted.
    pub truncated: bool,
    /// Schema revision used to build the result.
    pub schema_revision: u64,
    /// Source/document revision used to build the result.
    pub revision: u64,
    /// Result freshness. Successful queries always return `Fresh`.
    pub freshness: CompletionFreshness,
}

impl CompletionResult {
    /// True when no candidate survived the typed prefix filter.
    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }
}

/// A single value supplied by checked typed facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionValueFact {
    /// Exact source spelling to insert.
    pub value: String,
    /// Optional checked explanation for the value.
    pub detail: Option<String>,
    /// Optional explicit source/provenance label.
    pub source: Option<String>,
    /// Lower values sort first after prefix matching.
    pub rank: u32,
}

impl CompletionValueFact {
    /// Construct a value fact with the default typed-schema source.
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            detail: None,
            source: None,
            rank: 0,
        }
    }

    /// Attach a detail string.
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Attach an explicit source label.
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// Set the deterministic tie-break priority.
    pub fn with_rank(mut self, rank: u32) -> Self {
        self.rank = rank;
        self
    }
}

/// One typed value family projected for one checked CLI input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypedCompletionKind {
    /// Variants of one checked enum type.
    Enum {
        /// Fully qualified or local checked enum name.
        type_name: String,
        /// Declared variants in source order; the query owns final ordering.
        variants: Vec<CompletionValueFact>,
    },
    /// Values of one checked erased tag fact.
    Tag {
        /// Checked tag name.
        tag_name: String,
        /// Declared tag values.
        values: Vec<CompletionValueFact>,
    },
    /// Explicit literals accepted by one checked input.
    Literal {
        /// Checked input type or literal domain name.
        type_name: String,
        /// Literal spellings.
        values: Vec<CompletionValueFact>,
    },
    /// A boolean input. The engine supplies exactly `true` and `false`.
    Bool,
}

/// Typed completion facts produced by the checked CLI/LSP front end.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypedCompletionFact {
    /// `None` for root inputs, or the exact checked subcommand name.
    pub command: Option<String>,
    /// Canonical schema field name. Flags are accepted only as lookup syntax.
    pub field: String,
    /// One of the four supported typed value families.
    pub kind: TypedCompletionKind,
}

impl TypedCompletionFact {
    /// Construct enum variants for a root input.
    pub fn enum_values(
        field: impl Into<String>,
        type_name: impl Into<String>,
        variants: Vec<CompletionValueFact>,
    ) -> Self {
        Self {
            command: None,
            field: field.into(),
            kind: TypedCompletionKind::Enum {
                type_name: type_name.into(),
                variants,
            },
        }
    }

    /// Construct tag values for a root input.
    pub fn tag_values(
        field: impl Into<String>,
        tag_name: impl Into<String>,
        values: Vec<CompletionValueFact>,
    ) -> Self {
        Self {
            command: None,
            field: field.into(),
            kind: TypedCompletionKind::Tag {
                tag_name: tag_name.into(),
                values,
            },
        }
    }

    /// Construct checked literal values for a root input.
    pub fn literal_values(
        field: impl Into<String>,
        type_name: impl Into<String>,
        values: Vec<CompletionValueFact>,
    ) -> Self {
        Self {
            command: None,
            field: field.into(),
            kind: TypedCompletionKind::Literal {
                type_name: type_name.into(),
                values,
            },
        }
    }

    /// Construct a boolean fact for a root input.
    pub fn bool_value(field: impl Into<String>) -> Self {
        Self {
            command: None,
            field: field.into(),
            kind: TypedCompletionKind::Bool,
        }
    }

    /// Bind a fact to one checked subcommand.
    pub fn for_command(mut self, command: impl Into<String>) -> Self {
        self.command = Some(command.into());
        self
    }
}

/// Bounded query limits. Hard caps are applied even when a host supplies a
/// larger value, so a malformed or stale host request cannot turn completion
/// into an unbounded traversal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompletionLimits {
    /// Maximum returned candidates.
    pub max_candidates: usize,
    /// Maximum retained entries per inspected directory.
    pub max_directory_entries: usize,
    /// Maximum path components below an authority root.
    pub max_path_depth: usize,
}

impl Default for CompletionLimits {
    fn default() -> Self {
        Self {
            max_candidates: MAX_COMPLETION_CANDIDATES,
            max_directory_entries: MAX_DIRECTORY_ENTRIES,
            max_path_depth: MAX_PATH_DEPTH,
        }
    }
}

impl CompletionLimits {
    fn bounded(self) -> Result<Self, CompletionError> {
        if self.max_candidates == 0 {
            return Err(CompletionError::InvalidLimit {
                name: "max_candidates",
            });
        }
        if self.max_directory_entries == 0 {
            return Err(CompletionError::InvalidLimit {
                name: "max_directory_entries",
            });
        }
        if self.max_path_depth == 0 {
            return Err(CompletionError::InvalidLimit {
                name: "max_path_depth",
            });
        }
        Ok(Self {
            max_candidates: self.max_candidates.min(MAX_COMPLETION_CANDIDATES),
            max_directory_entries: self.max_directory_entries.min(MAX_DIRECTORY_ENTRIES),
            max_path_depth: self.max_path_depth.min(MAX_PATH_DEPTH),
        })
    }
}

/// Request facts supplied by a CLI, LSP, or shell adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionRequest {
    /// Source or command-line text containing the cursor.
    pub source: String,
    /// Cursor byte offset in `source`.
    pub cursor: usize,
    /// Semantic completion target selected by the host adapter.
    pub target: CompletionTarget,
    /// Canonical schema field for `Value` and `Path` targets.
    pub field: Option<String>,
    /// Exact checked subcommand, or `None` for root inputs.
    pub command: Option<String>,
    /// Schema revision the host used when selecting the target.
    pub schema_revision: u64,
    /// Source/document revision the host used when selecting the target.
    pub revision: u64,
}

impl CompletionRequest {
    /// Construct a value request with no selected field.
    ///
    /// A host should normally use [`Self::for_value`], [`Self::for_path`], or
    /// [`Self::for_jobs`] so the engine never has to infer CLI meaning from
    /// shell syntax.
    pub fn new(source: impl Into<String>, cursor: usize) -> Self {
        Self {
            source: source.into(),
            cursor,
            target: CompletionTarget::Value,
            field: None,
            command: None,
            schema_revision: 0,
            revision: 0,
        }
    }

    /// Construct a typed value request.
    pub fn for_value(
        source: impl Into<String>,
        cursor: usize,
        field: impl Into<String>,
        schema_revision: u64,
        revision: u64,
    ) -> Self {
        Self {
            source: source.into(),
            cursor,
            target: CompletionTarget::Value,
            field: Some(field.into()),
            command: None,
            schema_revision,
            revision,
        }
    }

    /// Construct a typed path request.
    pub fn for_path(
        source: impl Into<String>,
        cursor: usize,
        field: impl Into<String>,
        schema_revision: u64,
        revision: u64,
    ) -> Self {
        Self {
            source: source.into(),
            cursor,
            target: CompletionTarget::Path,
            field: Some(field.into()),
            command: None,
            schema_revision,
            revision,
        }
    }

    /// Construct a checked job-name request.
    pub fn for_jobs(
        source: impl Into<String>,
        cursor: usize,
        schema_revision: u64,
        revision: u64,
    ) -> Self {
        Self {
            source: source.into(),
            cursor,
            target: CompletionTarget::Jobs,
            field: None,
            command: None,
            schema_revision,
            revision,
        }
    }

    /// Select a checked subcommand without parsing shell syntax.
    pub fn in_command(mut self, command: impl Into<String>) -> Self {
        self.command = Some(command.into());
        self
    }
}

/// Explicit path authority. Roots are lexical, absolute, regular directories
/// and are validated without resolving symbolic links.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathAuthority {
    roots: Vec<PathBuf>,
}

impl PathAuthority {
    /// Grant read-only completion authority over absolute directories.
    pub fn new<I, P>(roots: I) -> Result<Self, PathAuthorityError>
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        let mut normalized = roots
            .into_iter()
            .map(Into::into)
            .map(|root| {
                if !root.is_absolute() {
                    return Err(PathAuthorityError::RelativeRoot(root));
                }
                normalize_absolute_path(&root).map_err(|reason| PathAuthorityError::InvalidRoot {
                    path: root,
                    reason,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if normalized.is_empty() {
            return Err(PathAuthorityError::Empty);
        }
        if normalized.len() > MAX_AUTHORITY_ROOTS {
            return Err(PathAuthorityError::TooManyRoots {
                limit: MAX_AUTHORITY_ROOTS,
            });
        }
        normalized.sort();
        normalized.dedup();
        for root in &normalized {
            match safe_metadata(root).map_err(|error| PathAuthorityError::Io {
                path: root.clone(),
                error: error.to_string(),
            })? {
                SafeMetadata::Present(metadata) if metadata.is_dir() => {}
                SafeMetadata::Present(_) => {
                    return Err(PathAuthorityError::NotDirectory(root.clone()));
                }
                SafeMetadata::Missing => {
                    return Err(PathAuthorityError::MissingRoot(root.clone()));
                }
                SafeMetadata::Symlink => {
                    return Err(PathAuthorityError::SymlinkRoot(root.clone()));
                }
            }
        }
        Ok(Self { roots: normalized })
    }

    /// Return the sorted, deduplicated authority roots.
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }
}

/// Authority construction failures.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PathAuthorityError {
    /// No root was supplied.
    Empty,
    /// A root was not absolute.
    RelativeRoot(PathBuf),
    /// A root path could not be normalized without escaping its filesystem root.
    InvalidRoot { path: PathBuf, reason: String },
    /// More roots were supplied than the bounded authority allows.
    TooManyRoots { limit: usize },
    /// The root does not exist.
    MissingRoot(PathBuf),
    /// The root exists but is not a directory.
    NotDirectory(PathBuf),
    /// The root itself is a symlink.
    SymlinkRoot(PathBuf),
    /// The host denied metadata access.
    Io { path: PathBuf, error: String },
}

impl std::fmt::Display for PathAuthorityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "path authority needs at least one root"),
            Self::RelativeRoot(path) => write!(f, "path authority root is not absolute: {}", path.display()),
            Self::InvalidRoot { path, reason } => {
                write!(f, "path authority root is invalid ({}): {}", path.display(), reason)
            }
            Self::TooManyRoots { limit } => {
                write!(f, "path authority has more than {limit} roots")
            }
            Self::MissingRoot(path) => write!(f, "path authority root does not exist: {}", path.display()),
            Self::NotDirectory(path) => write!(f, "path authority root is not a directory: {}", path.display()),
            Self::SymlinkRoot(path) => write!(f, "path authority root is a symbolic link: {}", path.display()),
            Self::Io { path, error } => write!(f, "could not inspect path authority root {}: {error}", path.display()),
        }
    }
}

impl std::error::Error for PathAuthorityError {}

/// Context facts consumed by the query engine.
pub struct CompletionContext<'a> {
    /// The existing checked CLI schema; this module does not replace it.
    pub schema: &'a CLICommandSchema,
    /// Current schema revision.
    pub schema_revision: u64,
    /// Current source/document revision.
    pub revision: u64,
    /// Checked enum/tag/literal/bool facts.
    pub typed_facts: &'a [TypedCompletionFact],
    /// Explicit filesystem authority, if path completion is permitted.
    pub path_authority: Option<&'a PathAuthority>,
    /// Bounded query policy.
    pub limits: CompletionLimits,
}

impl<'a> CompletionContext<'a> {
    /// Construct a context with no dynamic values and no filesystem authority.
    pub fn new(schema: &'a CLICommandSchema, schema_revision: u64, revision: u64) -> Self {
        Self {
            schema,
            schema_revision,
            revision,
            typed_facts: &[],
            path_authority: None,
            limits: CompletionLimits::default(),
        }
    }

    /// Supply checked typed facts.
    pub fn with_typed_facts(mut self, typed_facts: &'a [TypedCompletionFact]) -> Self {
        self.typed_facts = typed_facts;
        self
    }

    /// Supply explicit filesystem authority.
    pub fn with_path_authority(mut self, path_authority: &'a PathAuthority) -> Self {
        self.path_authority = Some(path_authority);
        self
    }

    /// Supply bounded query limits.
    pub fn with_limits(mut self, limits: CompletionLimits) -> Self {
        self.limits = limits;
        self
    }
}

/// Explicit, fail-closed completion errors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompletionError {
    /// The cursor is outside source bytes or splits a UTF-8 scalar.
    InvalidCursor { cursor: usize, source_len: usize },
    /// The request schema revision differs from the checked context.
    StaleSchema { expected: u64, actual: u64 },
    /// The request source revision differs from the checked context.
    StaleRevision { expected: u64, actual: u64 },
    /// A value/path request omitted its canonical schema field.
    MissingField { target: CompletionTarget },
    /// The named command does not exist in the checked schema.
    UnknownCommand { command: String },
    /// The named field/flag does not exist in the selected schema scope.
    UnknownField { command: Option<String>, field: String },
    /// Two checked facts claim the same field and scope.
    ConflictingFacts { command: Option<String>, field: String },
    /// The checked input cannot support the requested target.
    UnsupportedInput { field: String, kind: String, target: CompletionTarget },
    /// A path request has no explicit authority.
    MissingPathAuthority,
    /// A path request resolves outside every explicit root.
    PathOutsideAuthority { path: String },
    /// A path request exceeds the configured bounded depth.
    PathDepthExceeded { path: String, limit: usize },
    /// A checked fact is malformed.
    InvalidFact { field: String, reason: String },
    /// A caller supplied a zero query limit.
    InvalidLimit { name: &'static str },
    /// A bounded filesystem query failed after authority checks.
    Filesystem { path: PathBuf, error: String },
}

impl std::fmt::Display for CompletionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCursor { cursor, source_len } => {
                write!(f, "completion cursor {cursor} is invalid for {source_len} source bytes")
            }
            Self::StaleSchema { expected, actual } => {
                write!(f, "completion schema is stale (request {expected}, context {actual})")
            }
            Self::StaleRevision { expected, actual } => {
                write!(f, "completion source is stale (request {expected}, context {actual})")
            }
            Self::MissingField { target } => {
                write!(f, "completion target {} needs a checked field", target_name(*target))
            }
            Self::UnknownCommand { command } => write!(f, "unknown checked completion command `{command}`"),
            Self::UnknownField { command, field } => match command {
                Some(command) => write!(f, "unknown checked completion field `{field}` for `{command}`"),
                None => write!(f, "unknown checked completion field `{field}`"),
            },
            Self::ConflictingFacts { command, field } => match command {
                Some(command) => write!(f, "conflicting completion facts for `{command}.{field}`"),
                None => write!(f, "conflicting completion facts for `{field}`"),
            },
            Self::UnsupportedInput { field, kind, target } => write!(
                f,
                "checked input `{field}` has kind {kind}, which cannot serve {} completion",
                target_name(*target)
            ),
            Self::MissingPathAuthority => write!(f, "path completion needs explicit filesystem authority"),
            Self::PathOutsideAuthority { path } => {
                write!(f, "completion path is outside the granted roots: {path}")
            }
            Self::PathDepthExceeded { path, limit } => {
                write!(f, "completion path exceeds the {limit}-component bound: {path}")
            }
            Self::InvalidFact { field, reason } => {
                write!(f, "invalid completion fact for `{field}`: {reason}")
            }
            Self::InvalidLimit { name } => write!(f, "completion limit `{name}` must be non-zero"),
            Self::Filesystem { path, error } => {
                write!(f, "could not inspect completion path {}: {error}", path.display())
            }
        }
    }
}

impl std::error::Error for CompletionError {}

/// Query one typed/path/job completion surface.
///
/// The request carries semantic target selection and revisions. The context
/// carries the existing checked schema, explicit typed facts, and optional
/// path authority. No provider callback or command execution seam exists here.
pub fn query(
    request: &CompletionRequest,
    context: &CompletionContext<'_>,
) -> Result<CompletionResult, CompletionError> {
    if request.schema_revision != context.schema_revision {
        return Err(CompletionError::StaleSchema {
            expected: request.schema_revision,
            actual: context.schema_revision,
        });
    }
    if request.revision != context.revision {
        return Err(CompletionError::StaleRevision {
            expected: request.revision,
            actual: context.revision,
        });
    }
    if request.source.len() > MAX_COMPLETION_SOURCE_BYTES {
        return Err(CompletionError::InvalidCursor {
            cursor: request.cursor,
            source_len: request.source.len(),
        });
    }
    if request.cursor > request.source.len() || !request.source.is_char_boundary(request.cursor) {
        return Err(CompletionError::InvalidCursor {
            cursor: request.cursor,
            source_len: request.source.len(),
        });
    }
    let limits = context.limits.bounded()?;
    if context.typed_facts.len() > MAX_TYPED_FACTS {
        return Err(CompletionError::InvalidFact {
            field: "<context>".to_string(),
            reason: format!("more than {MAX_TYPED_FACTS} typed facts"),
        });
    }
    let token_span = token_span(&request.source, request.cursor);
    let value_start = value_start(
        &request.source,
        token_span,
        request.cursor,
        request.field.as_deref(),
    );
    let replacement_span = Span::new(value_start, request.cursor);
    let prefix = request.source[value_start..request.cursor].to_string();
    let (mut candidates, mut truncated) = match request.target {
        CompletionTarget::Jobs => (
            job_candidates(context.schema, replacement_span, &prefix),
            false,
        ),
        CompletionTarget::Path => {
            let field = request
                .field
                .as_deref()
                .ok_or(CompletionError::MissingField {
                    target: request.target,
                })?;
            let input = find_input(context.schema, request.command.as_deref(), field)?;
            if input.value_kind() != CLIValueKind::Path {
                return Err(CompletionError::UnsupportedInput {
                    field: input.field.clone(),
                    kind: input.value_kind().as_str().to_string(),
                    target: request.target,
                });
            }
            let authority = context
                .path_authority
                .ok_or(CompletionError::MissingPathAuthority)?;
            let projection = path_candidates(authority, &prefix, replacement_span, limits)?;
            (projection.candidates, projection.truncated)
        }
        CompletionTarget::Value => (
            value_candidates(request, context, replacement_span, &prefix)?,
            false,
        ),
    };
    candidates.sort_by(candidate_order);
    let mut seen = BTreeSet::<(CompletionKind, String)>::new();
    let mut candidates = candidates
        .into_iter()
        .filter(|candidate| seen.insert((candidate.kind, candidate.label.clone())))
        .collect::<Vec<_>>();
    if candidates.len() > limits.max_candidates {
        candidates.truncate(limits.max_candidates);
        truncated = true;
    }
    Ok(CompletionResult {
        token_span,
        prefix,
        replacement_span,
        candidates,
        truncated,
        schema_revision: context.schema_revision,
        revision: context.revision,
        freshness: CompletionFreshness::Fresh,
    })
}

fn value_candidates(
    request: &CompletionRequest,
    context: &CompletionContext<'_>,
    replacement_span: Span,
    prefix: &str,
) -> Result<Vec<CompletionCandidate>, CompletionError> {
    let field = request
        .field
        .as_deref()
        .ok_or(CompletionError::MissingField {
            target: request.target,
        })?;
    let input = find_input(context.schema, request.command.as_deref(), field)?;
    let fact = matching_fact(
        context.typed_facts,
        request.command.as_deref(),
        &input.field,
    )?;
    let Some(fact) = fact else {
        if input.value_kind() == CLIValueKind::Bool {
            return Ok(bool_candidates(replacement_span, prefix));
        }
        return Ok(Vec::new());
    };
    validate_typed_fact(fact)?;
    let (kind, detail, values) = match &fact.kind {
        TypedCompletionKind::Enum { type_name, variants } => {
            if input.value_kind() != CLIValueKind::String {
                return Err(CompletionError::UnsupportedInput {
                    field: input.field.clone(),
                    kind: input.value_kind().as_str().to_string(),
                    target: request.target,
                });
            }
            (
                CompletionKind::Enum,
                format!("enum {type_name}"),
                variants,
            )
        }
        TypedCompletionKind::Tag { tag_name, values } => {
            if input.value_kind() != CLIValueKind::String {
                return Err(CompletionError::UnsupportedInput {
                    field: input.field.clone(),
                    kind: input.value_kind().as_str().to_string(),
                    target: request.target,
                });
            }
            (CompletionKind::Tag, format!("tag {tag_name}"), values)
        }
        TypedCompletionKind::Literal { type_name, values } => (
            CompletionKind::Literal,
            format!("literal {type_name}"),
            values,
        ),
        TypedCompletionKind::Bool => {
            if input.value_kind() != CLIValueKind::Bool {
                return Err(CompletionError::UnsupportedInput {
                    field: input.field.clone(),
                    kind: input.value_kind().as_str().to_string(),
                    target: request.target,
                });
            }
            return Ok(bool_candidates(replacement_span, prefix));
        }
    };
    typed_candidates(
        kind,
        detail,
        values,
        replacement_span,
        prefix,
        &input.field,
    )
}

fn bool_candidates(span: Span, prefix: &str) -> Vec<CompletionCandidate> {
    ["false", "true"]
        .into_iter()
        .enumerate()
        .filter(|(_, value)| value.starts_with(prefix))
        .map(|(rank, value)| CompletionCandidate {
            label: value.to_string(),
            kind: CompletionKind::Bool,
            detail: "Boolean value".to_string(),
            replacement: CompletionReplacement::new(span, value),
            source: "typed-schema".to_string(),
            freshness: CompletionFreshness::Fresh,
            rank: match value == prefix {
                true => 0,
                false => 1_000 + rank as u32,
            },
        })
        .collect()
}

fn typed_candidates(
    kind: CompletionKind,
    detail: String,
    values: &[CompletionValueFact],
    span: Span,
    prefix: &str,
    field: &str,
) -> Result<Vec<CompletionCandidate>, CompletionError> {
    if values.len() > MAX_FACT_VALUES {
        return Err(CompletionError::InvalidFact {
            field: field.to_string(),
            reason: format!("more than {MAX_FACT_VALUES} values"),
        });
    }
    let mut candidates = Vec::new();
    for value in values {
        validate_fact_value(field, value)?;
        if !value.value.starts_with(prefix) {
            continue;
        }
        let match_rank = if value.value == prefix { 0 } else { 1 };
        candidates.push(CompletionCandidate {
            label: value.value.clone(),
            kind,
            detail: value
                .detail
                .clone()
                .unwrap_or_else(|| detail.clone()),
            replacement: CompletionReplacement::new(span, value.value.clone()),
            source: value
                .source
                .clone()
                .unwrap_or_else(|| "typed-schema".to_string()),
            freshness: CompletionFreshness::Fresh,
            rank: match_rank * 1_000_000 + value.rank.min(999_999),
        });
    }
    Ok(candidates)
}

fn validate_typed_fact(fact: &TypedCompletionFact) -> Result<(), CompletionError> {
    if fact.field.is_empty() || fact.field.chars().any(char::is_control) {
        return Err(CompletionError::InvalidFact {
            field: fact.field.clone(),
            reason: "field is empty or contains a control character".to_string(),
        });
    }
    if fact
        .command
        .as_deref()
        .is_some_and(|command| command.is_empty() || command.chars().any(char::is_control))
    {
        return Err(CompletionError::InvalidFact {
            field: fact.field.clone(),
            reason: "command is empty or contains a control character".to_string(),
        });
    }
    match &fact.kind {
        TypedCompletionKind::Enum {
            type_name,
            variants,
        } => {
            validate_fact_name(&fact.field, type_name, "enum type")?;
            validate_fact_values(&fact.field, variants)?;
        }
        TypedCompletionKind::Tag { tag_name, values } => {
            validate_fact_name(&fact.field, tag_name, "tag name")?;
            validate_fact_values(&fact.field, values)?;
        }
        TypedCompletionKind::Literal { type_name, values } => {
            validate_fact_name(&fact.field, type_name, "literal type")?;
            validate_fact_values(&fact.field, values)?;
        }
        TypedCompletionKind::Bool => {}
    }
    Ok(())
}

fn validate_fact_name(field: &str, value: &str, label: &str) -> Result<(), CompletionError> {
    if value.is_empty()
        || value.len() > MAX_COMPLETION_SOURCE_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(CompletionError::InvalidFact {
            field: field.to_string(),
            reason: format!("{label} is empty, too large, or contains a control character"),
        });
    }
    Ok(())
}

fn validate_fact_values(
    field: &str,
    values: &[CompletionValueFact],
) -> Result<(), CompletionError> {
    if values.len() > MAX_FACT_VALUES {
        return Err(CompletionError::InvalidFact {
            field: field.to_string(),
            reason: format!("more than {MAX_FACT_VALUES} values"),
        });
    }
    for value in values {
        validate_fact_value(field, value)?;
    }
    Ok(())
}

fn validate_fact_value(field: &str, value: &CompletionValueFact) -> Result<(), CompletionError> {
    if value.value.is_empty() || value.value.len() > MAX_COMPLETION_SOURCE_BYTES {
        return Err(CompletionError::InvalidFact {
            field: field.to_string(),
            reason: "value is empty or too large".to_string(),
        });
    }
    if value.value.chars().any(char::is_control) {
        return Err(CompletionError::InvalidFact {
            field: field.to_string(),
            reason: "value contains a control character".to_string(),
        });
    }
    if value
        .detail
        .as_deref()
        .is_some_and(|detail| {
            detail.len() > MAX_COMPLETION_SOURCE_BYTES || detail.chars().any(char::is_control)
        })
    {
        return Err(CompletionError::InvalidFact {
            field: field.to_string(),
            reason: "detail is too large or contains a control character".to_string(),
        });
    }
    if value
        .source
        .as_deref()
        .is_some_and(|source| {
            source.len() > MAX_COMPLETION_SOURCE_BYTES || source.chars().any(char::is_control)
        })
    {
        return Err(CompletionError::InvalidFact {
            field: field.to_string(),
            reason: "source is too large or contains a control character".to_string(),
        });
    }
    Ok(())
}

fn job_candidates(
    schema: &CLICommandSchema,
    replacement_span: Span,
    prefix: &str,
) -> Vec<CompletionCandidate> {
    schema
        .jobs
        .iter()
        .filter(|job| job.scope_name() != "internal" && job.name.starts_with(prefix))
        .map(|job| CompletionCandidate {
            label: job.name.clone(),
            kind: CompletionKind::Job,
            detail: job
                .doc
                .clone()
                .unwrap_or_else(|| format!("{} job", job.scope_name())),
            replacement: CompletionReplacement::new(replacement_span, job.name.clone()),
            source: "job-schema".to_string(),
            freshness: CompletionFreshness::Fresh,
            rank: if job.name == prefix { 0 } else { 1 },
        })
        .collect()
}

fn find_input<'a>(
    schema: &'a CLICommandSchema,
    command: Option<&str>,
    field: &str,
) -> Result<&'a CLIInputSchema, CompletionError> {
    let inputs = if let Some(command_name) = command {
        let command_schema = schema
            .commands
            .iter()
            .find(|candidate| candidate.name == command_name)
            .ok_or_else(|| CompletionError::UnknownCommand {
                command: command_name.to_string(),
            })?;
        &command_schema.inputs
    } else {
        &schema.inputs
    };
    let wanted = field
        .strip_prefix("--")
        .or_else(|| field.strip_prefix('-'))
        .unwrap_or(field);
    inputs
        .iter()
        .find(|input| {
            input.field == field
                || input.flag == wanted
                || input.short.as_deref() == Some(wanted)
        })
        .ok_or_else(|| CompletionError::UnknownField {
            command: command.map(str::to_string),
            field: field.to_string(),
        })
}

fn matching_fact<'a>(
    typed_facts: &'a [TypedCompletionFact],
    command: Option<&str>,
    field: &str,
) -> Result<Option<&'a TypedCompletionFact>, CompletionError> {
    let mut matches = typed_facts
        .iter()
        .filter(|fact| fact.field == field && fact.command.as_deref() == command);
    let first = matches.next();
    if matches.next().is_some() {
        return Err(CompletionError::ConflictingFacts {
            command: command.map(str::to_string),
            field: field.to_string(),
        });
    }
    Ok(first)
}
fn candidate_order(left: &CompletionCandidate, right: &CompletionCandidate) -> Ordering {
    let rank = left.rank.cmp(&right.rank);
    if rank != Ordering::Equal {
        return rank;
    }
    // Typed facts are declarations, not a lexical filesystem listing:
    // equal-ranked values keep the order supplied by the checked schema.
    // `sort_by` is stable, so returning Equal here preserves that order.
    if left.kind == right.kind && preserves_declaration_order(left.kind) {
        return Ordering::Equal;
    }
    left.label
        .cmp(&right.label)
        .then_with(|| left.kind.cmp(&right.kind))
        .then_with(|| left.detail.cmp(&right.detail))
        .then_with(|| left.source.cmp(&right.source))
}

const fn preserves_declaration_order(kind: CompletionKind) -> bool {
    matches!(
        kind,
        CompletionKind::Enum
            | CompletionKind::Tag
            | CompletionKind::Literal
            | CompletionKind::Bool
    )
}

fn target_name(target: CompletionTarget) -> &'static str {
    match target {
        CompletionTarget::Value => "value",
        CompletionTarget::Path => "path",
        CompletionTarget::Jobs => "job",
    }
}

fn token_span(source: &str, cursor: usize) -> Span {
    let mut start = 0;
    for (index, character) in source[..cursor].char_indices().rev() {
        if character.is_whitespace() {
            start = index + character.len_utf8();
            break;
        }
    }
    let mut end = source.len();
    for (index, character) in source[cursor..].char_indices() {
        if character.is_whitespace() {
            end = cursor + index;
            break;
        }
    }
    Span::new(start, end)
}

fn value_start(source: &str, token: Span, cursor: usize, field: Option<&str>) -> usize {
    let Some(field) = field else {
        return token.start;
    };
    let token_text = &source[token.start..cursor];
    let Some(equals) = token_text.rfind('=') else {
        return token.start;
    };
    let head = &token_text[..equals];
    let wanted = field
        .strip_prefix("--")
        .or_else(|| field.strip_prefix('-'))
        .unwrap_or(field);
    let matches_field = head == field
        || head == format!("--{wanted}")
        || head == format!("-{wanted}");
    if matches_field {
        token.start + equals + 1
    } else {
        token.start
    }
}

fn normalize_absolute_path(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("path is not absolute".to_string());
    }
    normalize_lexical_path(path).map_err(|reason| reason.to_string())
}

fn normalize_lexical_path(path: &Path) -> Result<PathBuf, &'static str> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::Normal(value) => normalized.push(value),
            Component::ParentDir => {
                if !normalized.pop() && !path.is_absolute() {
                    return Err("parent traversal escapes the authority root");
                }
            }
        }
    }
    Ok(normalized)
}

#[derive(Debug)]
enum SafeMetadata {
    Present(fs::Metadata),
    Missing,
    Symlink,
}

fn safe_metadata(path: &Path) -> io::Result<SafeMetadata> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Ok(SafeMetadata::Symlink);
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(SafeMetadata::Missing);
            }
            Err(error) => return Err(error),
        }
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Ok(SafeMetadata::Symlink),
        Ok(metadata) => Ok(SafeMetadata::Present(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(SafeMetadata::Missing),
        Err(error) => Err(error),
    }
}

struct PathProjection {
    candidates: Vec<CompletionCandidate>,
    truncated: bool,
}

fn path_candidates(
    authority: &PathAuthority,
    prefix: &str,
    replacement_span: Span,
    limits: CompletionLimits,
) -> Result<PathProjection, CompletionError> {
    if prefix.chars().any(char::is_control) {
        return Err(CompletionError::PathOutsideAuthority {
            path: prefix.to_string(),
        });
    }
    let path = Path::new(prefix);
    let normalized = normalize_lexical_path(path).map_err(|_| {
        CompletionError::PathOutsideAuthority {
            path: prefix.to_string(),
        }
    })?;
    let components = normalized.components().count();
    if components > limits.max_path_depth {
        return Err(CompletionError::PathDepthExceeded {
            path: prefix.to_string(),
            limit: limits.max_path_depth,
        });
    }
    let trailing_separator = prefix.ends_with(std::path::MAIN_SEPARATOR);
    let parent_prefix = if trailing_separator {
        prefix
    } else {
        prefix
            .rfind(std::path::MAIN_SEPARATOR)
            .map(|index| &prefix[..index + 1])
            .unwrap_or_default()
    };
    let filter = if trailing_separator {
        ""
    } else {
        prefix
            .rsplit(std::path::MAIN_SEPARATOR)
            .next()
            .unwrap_or(prefix)
    };
    let parent_path = if trailing_separator {
        normalized.clone()
    } else {
        normalized
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default()
    };
    let parent_is_absolute = parent_path.is_absolute();
    let roots = if parent_is_absolute {
        let matches = authority
            .roots
            .iter()
            .filter(|root| parent_path.starts_with(root.as_path()))
            .collect::<Vec<_>>();
        if matches.is_empty() {
            return Err(CompletionError::PathOutsideAuthority {
                path: prefix.to_string(),
            });
        }
        matches
    } else {
        authority.roots.iter().collect::<Vec<_>>()
    };
    let mut entries = BTreeMap::<String, PathEntry>::new();
    let mut truncated = false;
    for (root_index, root) in roots.into_iter().enumerate() {
        let directory = if parent_is_absolute {
            parent_path.to_path_buf()
        } else {
            root.join(&parent_path)
        };
        let Some(rows) = read_directory(&directory, limits.max_directory_entries)? else {
            continue;
        };
        if rows.truncated {
            truncated = true;
        }
        for entry in rows.entries {
            if !entry.name.starts_with(filter) {
                continue;
            }
            let label = replacement_label(parent_prefix, &entry.name, entry.is_dir);
            let candidate = PathEntry {
                label: label.clone(),
                detail: if entry.is_dir {
                    "directory".to_string()
                } else {
                    "file".to_string()
                },
                rank: root_index as u32,
                is_dir: entry.is_dir,
            };
            entries
                .entry(label)
                .and_modify(|current| {
                    if path_entry_order(&candidate, current) == Ordering::Less {
                        *current = candidate.clone();
                    }
                })
                .or_insert(candidate);
        }
    }
    let mut candidates = entries
        .into_values()
        .map(|entry| CompletionCandidate {
            label: entry.label.clone(),
            kind: CompletionKind::Path,
            detail: entry.detail,
            replacement: CompletionReplacement::new(replacement_span, entry.label),
            source: "filesystem".to_string(),
            freshness: CompletionFreshness::Fresh,
            rank: entry.rank,
        })
        .collect::<Vec<_>>();
    candidates.sort_by(candidate_order);
    Ok(PathProjection {
        candidates,
        truncated,
    })
}

fn replacement_label(parent_prefix: &str, name: &str, is_dir: bool) -> String {
    let mut label =
        String::with_capacity(parent_prefix.len() + name.len() + usize::from(is_dir));
    label.push_str(parent_prefix);
    label.push_str(name);
    if is_dir {
        label.push(std::path::MAIN_SEPARATOR);
    }
    label
}


#[derive(Clone, Debug)]
struct PathEntry {
    label: String,
    detail: String,
    rank: u32,
    is_dir: bool,
}

fn path_entry_order(left: &PathEntry, right: &PathEntry) -> Ordering {
    left.rank
        .cmp(&right.rank)
        .then_with(|| right.is_dir.cmp(&left.is_dir))
        .then_with(|| left.label.cmp(&right.label))
        .then_with(|| left.detail.cmp(&right.detail))
}

struct DirectoryRows {
    entries: Vec<DirectoryEntry>,
    truncated: bool,
}

struct DirectoryEntry {
    name: String,
    is_dir: bool,
}

fn read_directory(
    directory: &Path,
    limit: usize,
) -> Result<Option<DirectoryRows>, CompletionError> {
    let metadata = safe_metadata(directory).map_err(|error| CompletionError::Filesystem {
        path: directory.to_path_buf(),
        error: error.to_string(),
    })?;
    let SafeMetadata::Present(metadata) = metadata else {
        return Ok(None);
    };
    if !metadata.is_dir() {
        return Ok(None);
    }
    let mut entries = BTreeMap::<String, DirectoryEntry>::new();
    let mut truncated = false;
    let read = fs::read_dir(directory).map_err(|error| CompletionError::Filesystem {
        path: directory.to_path_buf(),
        error: error.to_string(),
    })?;
    for item in read {
        let item = item.map_err(|error| CompletionError::Filesystem {
            path: directory.to_path_buf(),
            error: error.to_string(),
        })?;
        let name = item.file_name();
        let Some(name_text) = name.to_str() else {
            continue;
        };
        let item_path = item.path();
        let metadata = match fs::symlink_metadata(&item_path) {
            Ok(metadata) if metadata.file_type().is_symlink() => continue,
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(_) => continue,
        };
        if !metadata.is_dir() && !metadata.is_file() {
            continue;
        }
        let entry = DirectoryEntry {
            name: name_text.to_string(),
            is_dir: metadata.is_dir(),
        };
        if entries.len() >= limit && !entries.contains_key(name_text) {
            let Some(last) = entries.keys().next_back().cloned() else {
                continue;
            };
            if name_text >= last.as_str() {
                truncated = true;
                continue;
            }
            entries.remove(&last);
            truncated = true;
        }
        entries.insert(name_text.to_string(), entry);
    }
    Ok(Some(DirectoryRows {
        entries: entries.into_values().collect(),
        truncated,
    }))
}


#[cfg(test)]
mod tests {
    use super::*;
    use jet_foundation::CLISchema::{CLIInputSchema, CLIInputShape};

    fn schema(kind: CLIValueKind, field: &str) -> CLICommandSchema {
        CLICommandSchema {
            entry_type: "Args".to_string(),
            description: None,
            inputs: vec![CLIInputSchema {
                field: field.to_string(),
                flag: field.replace('_', "-"),
                short: None,
                env: None,
                help: String::new(),
                metavar: Some(field.to_uppercase()),
                shape: CLIInputShape::Value {
                    kind,
                    optional: false,
                    default: None,
                },
                positional: Some(0),
            }],
            commands: Vec::new(),
            jobs: Vec::new(),
            standard: false,
            version: None,
        }
    }

    #[test]
    fn enum_completion_is_typed_filtered_and_span_exact() {
        let schema = schema(CLIValueKind::String, "mode");
        let facts = [TypedCompletionFact::enum_values(
            "mode",
            "Mode",
            vec![CompletionValueFact::new("fast"), CompletionValueFact::new("safe")],
        )];
        let context = CompletionContext::new(&schema, 3, 7).with_typed_facts(&facts);
        let request = CompletionRequest::for_value("--mode=sa", 9, "mode", 3, 7);
        let result = query(&request, &context).expect("completion");
        assert_eq!(result.token_span, Span::new(0, 9));
        assert_eq!(result.replacement_span, Span::new(7, 9));
        assert_eq!(result.prefix, "sa");
        assert_eq!(result.candidates[0].label, "safe");
    }

    #[test]
    fn stale_revisions_fail_closed() {
        let schema = schema(CLIValueKind::Bool, "verbose");
        let context = CompletionContext::new(&schema, 2, 4);
        let request = CompletionRequest::for_value("t", 1, "verbose", 1, 4);
        assert!(matches!(
            query(&request, &context),
            Err(CompletionError::StaleSchema { .. })
        ));
    }
}
