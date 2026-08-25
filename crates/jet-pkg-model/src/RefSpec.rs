//! Jetpack ref classifier: `name['@'source]['#'selector]` or
//! `source.package` (D-JPK-REF1=A, D-MONOREF1=A).
//!
//! A ref reads like an address: the package, its source, then its version or
//! channel. Bare `./`, `../`, and `/` paths need no provider word.
//! Examples:
//!   `fastfetch@nixpkgs`
//!   `halcyonomega/my-fastfetch-jet-config@github`
//!   `./my-env`

use crate::Syntax;
use std::collections::BTreeMap;

/// Where a ref is resolved from. The three built-in sources need no
/// declaration; `Named` is a source declared in an `env.jet` (D-JPK17) that
/// resolves to an upstream/pin via a `SourceTable`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The canonical Jetpack package catalog (D-JPK-SNIXREUSE1).
    Jetpack,
    /// The nixpkgs collection, realized through the Nix provider.
    ///
    /// Retained as an internal migration alias; its locked identity is
    /// canonicalized to `jetpack`.
    Nixpkgs,
    /// A GitHub repo holding an `env.jet` (or a translatable `flake.nix`).
    Github,
    /// A local directory holding a pack file or used as a flake fallback.
    Path,
    /// A package from the CRAN R package registry (D-FFI-R1).
    Cran,
    /// A package from the LuaRocks registry (D-FFI-LUA1).
    LuaRocks,
    /// A package from RubyGems (D-FFI-RUBY1).
    RubyGems,
    /// A distribution from CPAN (D-FFI-PERL1).
    Cpan,
    /// A package from Packagist (D-FFI-PHP1).
    Packagist,
    /// A package from Jet's first-party registry (D-JPK-REGISTRY1).
    JetRegistry,
    /// A package from npm (D-JPK-PROVIDERS2).
    Npm,
    /// A package from Cargo (D-JPK-PROVIDERS2).
    Cargo,
    /// A distribution from PyPI (D-JPK-EXTPROV1).
    PyPI,
    /// A package from Swift Package Manager (D-JPK-EXTPROV1).
    SwiftPM,
    /// A verified release artifact admitted directly by Jetpack (card #2166).
    Releases,
    /// A pack-declared named source, e.g. `stable` → a pinned nixpkgs (D-JPK17).
    Named(String),
}

impl Source {
    /// The source token as written after the `@` in a ref.
    pub fn label(&self) -> &str {
        match self {
            Source::Jetpack => Syntax::REF_SOURCE_JETPACK,
            Source::Nixpkgs => Syntax::REF_SOURCE_NIXPKGS,
            Source::Github => Syntax::REF_SOURCE_GITHUB,
            Source::Path => Syntax::REF_SOURCE_PATH,
            Source::Cran => Syntax::REF_SOURCE_CRAN,
            Source::LuaRocks => Syntax::REF_SOURCE_LUAROCKS,
            Source::RubyGems => Syntax::REF_SOURCE_RUBY,
            Source::Cpan => Syntax::REF_SOURCE_PERL,
            Source::Packagist => Syntax::REF_SOURCE_PHP,
            Source::JetRegistry => Syntax::REF_SOURCE_JET_REGISTRY,
            Source::Npm => Syntax::REF_SOURCE_NPM,
            Source::Cargo => Syntax::REF_SOURCE_CARGO,
            Source::PyPI => Syntax::REF_SOURCE_PYPI,
            Source::SwiftPM => Syntax::REF_SOURCE_SWIFTPM,
            Source::Releases => Syntax::REF_SOURCE_RELEASES,
            Source::Named(name) => name,
        }
    }

    /// Whether `name` is one of the built-in source keywords. Reads
    /// `Syntax::REF_SOURCE_PROVIDERS`, the one home for this set — never
    /// hand-copy the list here.
    pub fn is_builtin(name: &str) -> bool {
        Syntax::REF_SOURCE_PROVIDERS.contains(&name)
    }

    fn builtin(name: &str) -> Option<Source> {
        match name {
            n if n == Syntax::REF_SOURCE_JETPACK => Some(Source::Jetpack),
            n if n == Syntax::REF_SOURCE_NIXPKGS => Some(Source::Nixpkgs),
            n if n == Syntax::REF_SOURCE_GITHUB => Some(Source::Github),
            n if n == Syntax::REF_SOURCE_PATH => Some(Source::Path),
            n if n == Syntax::REF_SOURCE_CRAN => Some(Source::Cran),
            n if n == Syntax::REF_SOURCE_LUAROCKS => Some(Source::LuaRocks),
            n if n == Syntax::REF_SOURCE_RUBY => Some(Source::RubyGems),
            n if n == Syntax::REF_SOURCE_PERL => Some(Source::Cpan),
            n if n == Syntax::REF_SOURCE_PHP => Some(Source::Packagist),
            n if n == Syntax::REF_SOURCE_JET_REGISTRY => Some(Source::JetRegistry),
            n if n == Syntax::REF_SOURCE_NPM => Some(Source::Npm),
            n if n == Syntax::REF_SOURCE_CARGO => Some(Source::Cargo),
            n if n == Syntax::REF_SOURCE_PYPI => Some(Source::PyPI),
            n if n == Syntax::REF_SOURCE_SWIFTPM => Some(Source::SwiftPM),
            n if n == Syntax::REF_SOURCE_RELEASES => Some(Source::Releases),
            _ => None,
        }
    }
}

/// Which backend realizes a source: the `nix` compatibility provider, the
/// first-party `core` provider, or an explicit external provider boundary.
/// Unknown named-source declarations still default to `nix` (R1 behavior),
/// while direct external roots retain their concrete kind for fail-closed
/// dispatch.
///
/// `Infer` is a third, *unresolved* state used by the typed surface (U9): a
/// `…@github` source's kind can't be known during pure `evaluate_env`
/// evaluation — it depends on whether the remote repo carries a `package.jet`,
/// which only a realize-time probe (with the offline flag + source cache) can
/// answer. `Provider::resolve_kind` turns `Infer` into a concrete `Nix`/`Core`
/// when realization runs; it never reaches a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProviderKind {
    #[default]
    Nix,
    Core,
    Cran,
    LuaRocks,
    RubyGems,
    Cpan,
    Packagist,
    JetRegistry,
    Npm,
    Cargo,
    PyPI,
    SwiftPM,
    /// A native Jetpack release-artifact recipe.
    JetPackage,
    /// Decide `Nix` vs `Core` at realize time by peeking the source's
    /// `package.jet` (U9). Only the typed `…@github` surface produces this.
    Infer,
}

impl ProviderKind {
    /// Parse a provider name from a source declaration's third argument.
    /// Recognized direct ecosystem roots retain their concrete kind; unknown
    /// names remain the default `nix` for named-source inference.
    pub fn parse(s: &str) -> ProviderKind {
        match s {
            "core" => ProviderKind::Core,
            "cran" => ProviderKind::Cran,
            "luarocks" => ProviderKind::LuaRocks,
            "ruby" => ProviderKind::RubyGems,
            "perl" => ProviderKind::Cpan,
            "php" => ProviderKind::Packagist,
            "jet-registry" => ProviderKind::JetRegistry,
            "npm" => ProviderKind::Npm,
            "cargo" => ProviderKind::Cargo,
            "pypi" => ProviderKind::PyPI,
            "swiftpm" => ProviderKind::SwiftPM,
            "jetpackage" => ProviderKind::JetPackage,
            _ => ProviderKind::Nix,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ProviderKind::Nix => "nix",
            ProviderKind::Core => "core",
            ProviderKind::Cran => "cran",
            ProviderKind::LuaRocks => "luarocks",
            ProviderKind::RubyGems => "ruby",
            ProviderKind::Cpan => "perl",
            ProviderKind::Packagist => "php",
            ProviderKind::JetRegistry => "jet-registry",
            ProviderKind::Npm => "npm",
            ProviderKind::Cargo => "cargo",
            ProviderKind::PyPI => "pypi",
            ProviderKind::SwiftPM => "swiftpm",
            ProviderKind::JetPackage => "jetpackage",
            // Never user-shown: resolved before any listing/diagnostic.
            ProviderKind::Infer => "infer",
        }
    }
}

#[derive(Debug, Clone)]
struct SourceEntry {
    upstream: String,
    via: ProviderKind,
    policy: ChannelPolicy,
    raw: String,
}

/// D-CHANNEL-AUTO1=A: who is allowed to move a source pin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChannelPolicy {
    /// No channel marker: the declaration never moves.
    #[default]
    Pinned,
    /// Existing channel declarations move only through `jetpack update`.
    Manual,
    /// `#auto`: realization refreshes the channel and writes the exact pin back.
    Automatic,
}

impl ChannelPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pinned => "pinned",
            Self::Manual => "manual",
            Self::Automatic => "automatic",
        }
    }

    pub fn moves(self) -> bool {
        !matches!(self, Self::Pinned)
    }
}

/// The named sources an `env.jet` declares (D-JPK17): name → upstream/pin and
/// the provider that realizes it. Built-in sources are always resolvable and
/// are not stored here.
#[derive(Debug, Clone, Default)]
pub struct SourceTable {
    named: BTreeMap<String, SourceEntry>,
}

impl SourceTable {
    /// A table with no declared sources — only the built-ins resolve. This is
    /// the table for direct CLI refs (`jetpack use fastfetch@nixpkgs`).
    pub fn empty() -> SourceTable {
        SourceTable::default()
    }

    /// Build from `(name, upstream, provider)` declarations.
    pub fn from_decls<I>(decls: I) -> SourceTable
    where
        I: IntoIterator<Item = (String, String, ProviderKind)>,
    {
        SourceTable {
            named: decls
                .into_iter()
                .map(|(name, upstream, via)| {
                    let policy = policy_for_upstream(&upstream);
                    let raw = upstream.clone();
                    (
                        name,
                        SourceEntry {
                            upstream,
                            via,
                            policy,
                            raw,
                        },
                    )
                })
                .collect(),
        }
    }

    /// Add a source declaration when a CLI surface supplies a built-in native
    /// catalog entry alongside project declarations.
    pub fn ensure_decl(
        &mut self,
        name: impl Into<String>,
        upstream: impl Into<String>,
        via: ProviderKind,
    ) {
        let name = name.into();
        self.named.entry(name).or_insert_with(|| {
            let upstream = upstream.into();
            SourceEntry {
                policy: policy_for_upstream(&upstream),
                raw: upstream.clone(),
                upstream,
                via,
            }
        });
    }

    /// Set the authoring policy and raw source spelling for a declaration.
    /// `SourceTable::from_decls` remains the compatibility constructor for
    /// callers that only have the resolved upstream form.
    pub fn set_channel_metadata(
        &mut self,
        name: &str,
        policy: ChannelPolicy,
        raw: impl Into<String>,
    ) {
        if let Some(entry) = self.named.get_mut(name) {
            entry.policy = policy;
            entry.raw = raw.into();
        }
    }

    /// The upstream/pin a declared name resolves to, if any.
    pub fn upstream(&self, name: &str) -> Option<&str> {
        self.named.get(name).map(|e| e.upstream.as_str())
    }

    /// The provider a declared name uses (defaults to `nix` if undeclared).
    pub fn provider(&self, name: &str) -> ProviderKind {
        self.named.get(name).map(|e| e.via).unwrap_or_default()
    }

    /// The source's movement policy. Undeclared names are pinned.
    pub fn channel_policy(&self, name: &str) -> ChannelPolicy {
        self.named.get(name).map(|e| e.policy).unwrap_or_default()
    }

    /// The original source spelling used for manifest writeback.
    pub fn source_ref(&self, name: &str) -> Option<&str> {
        self.named.get(name).map(|e| e.raw.as_str())
    }

    /// Declared names, sorted — used to make "unknown source" errors helpful.
    pub fn declared_names(&self) -> Vec<String> {
        self.named.keys().cloned().collect()
    }

    /// Declared source records, sorted by name. Used by channel-locking verbs
    /// to walk source refs without exposing the map itself.
    pub fn declarations(&self) -> Vec<(String, String, ProviderKind)> {
        self.named
            .iter()
            .map(|(name, e)| (name.clone(), e.upstream.clone(), e.via))
            .collect()
    }

    /// Replace one declared source's upstream after reading an exact lock entry.
    pub fn set_upstream(&mut self, name: &str, upstream: String) {
        if let Some(entry) = self.named.get_mut(name) {
            entry.upstream = upstream;
        }
    }

    /// One `name=upstream@provider` line per declared source, in name order
    /// (the `BTreeMap` is already sorted). U19: folded into the trust-gate's
    /// env-definition hash, so re-pointing a named source (even with an
    /// unchanged package list) counts as a change and re-prompts.
    pub fn trust_lines(&self) -> Vec<String> {
        self.named
            .iter()
            .map(|(name, e)| format!("{name}={}@{:?}", e.upstream, e.via))
            .collect()
    }
}

/// A classified ref. `package` is the target before the final `@` — an attr
/// name for nixpkgs or an `owner/repo[/subpath]` for GitHub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefSpec {
    pub source: Source,
    pub package: String,
    pub raw: String,
}

impl RefSpec {
    /// A short, human display name for the package (last path segment).
    pub fn short_name(&self) -> &str {
        let package = self
            .package
            .split_once(Syntax::REF_CHANNEL_MARKER)
            .map(|(name, _)| name)
            .unwrap_or(&self.package);
        package.rsplit('/').next().unwrap_or(package)
    }
}

/// Why a ref string could not be classified. Each variant maps to a friendly
/// diagnostic (see `Output::ref_error`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefError {
    /// No `@` separator at all, e.g. `fastfetch`.
    MissingSeparator(String),
    /// An empty source or package half, e.g. `@nixpkgs` or `fastfetch@`.
    EmptyHalf(String),
    /// A retired provider-first ref that must be flipped, never reinterpreted.
    ProviderFirst { raw: String, replacement: String },
    /// A package ref uses one of the retired selector positions.
    NonCanonical { raw: String, replacement: String },
    /// The public `nixpkgs` source spelling retired; use `jetpack` exactly.
    RetiredNixpkgs { raw: String, replacement: String },
    /// The `path` provider word retired; local paths are bare.
    PathProviderRetired { raw: String, path: String },
    /// The source suffix is neither a built-in nor a declared named source.
    /// `declared` lists the pack's named sources so the message can help.
    UnknownSource {
        source: String,
        raw: String,
        declared: Vec<String>,
    },
    /// D-MONOREF1=A: bare name with no source suffix, and no workspace member
    /// matches, or the match was ambiguous.
    AmbiguousBare(String),
    /// E1230 (D-MONOREF1=A): a bare or path-form ref matched more than one
    /// workspace member. `candidates` are the members' relative paths, so the
    /// message can point at the exact addresses to disambiguate.
    AmbiguousMember {
        query: String,
        candidates: Vec<String>,
    },
    /// E1231 (D-MONOREF1=A): a bare or path-form ref matched no workspace
    /// member. `suggestions` are the closest member names/paths, for a
    /// did-you-mean.
    UnknownMember {
        query: String,
        suggestions: Vec<String>,
    },
}

impl RefError {
    /// The registered diagnostic code for the errors that carry one.
    pub fn code(&self) -> Option<&'static str> {
        match self {
            RefError::ProviderFirst { .. }
            | RefError::NonCanonical { .. }
            | RefError::RetiredNixpkgs { .. }
            | RefError::PathProviderRetired { .. } => Some("E1317"),
            RefError::AmbiguousMember { .. } => Some("E1230"),
            RefError::UnknownMember { .. } => Some("E1231"),
            _ => None,
        }
    }
}

/// A queryable index of the current workspace's members (Slice B). Built once
/// from a `WorkspacePlan` (or the `.jet/lock` mirror) and consulted by
/// `classify_with_workspace` to resolve path-form (`infra/logging`) and
/// bare-form (`logging`) addressing — the forms that have no `source:` prefix.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceIndex {
    /// `(member name, normalized relative path)`, in workspace source order.
    members: Vec<(String, String)>,
}

impl WorkspaceIndex {
    /// An index with no members — every bare/path ref is then `UnknownMember`.
    pub fn empty() -> WorkspaceIndex {
        WorkspaceIndex::default()
    }

    /// Build from `(name, path)` pairs (a `WorkspacePlan`'s members). Paths are
    /// normalized (leading `./` and trailing `/` trimmed) so a query and a
    /// stored path compare byte-for-byte regardless of how each was spelled.
    pub fn from_members<I>(members: I) -> WorkspaceIndex
    where
        I: IntoIterator<Item = (String, String)>,
    {
        WorkspaceIndex {
            members: members
                .into_iter()
                .map(|(name, path)| (name, normalize_member_path(&path)))
                .collect(),
        }
    }

    /// True when the index has no members (no workspace, or an empty one).
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// Member names + paths, for did-you-mean suggestion lists.
    fn candidate_labels(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for (name, path) in &self.members {
            out.push(name.clone());
            if path != name {
                out.push(path.clone());
            }
        }
        out
    }

    /// Resolve a path-form ref (`infra/logging`) to the matching member path.
    fn by_path(&self, query: &str) -> Vec<&str> {
        let q = normalize_member_path(query);
        self.members
            .iter()
            .filter(|(_, path)| *path == q)
            .map(|(_, path)| path.as_str())
            .collect()
    }

    /// Resolve a bare-form ref (`logging`) against member names.
    fn by_name(&self, query: &str) -> Vec<&str> {
        self.members
            .iter()
            .filter(|(name, _)| name == query)
            .map(|(_, path)| path.as_str())
            .collect()
    }
}

/// Normalize a workspace member path: trim a leading `./` and any trailing `/`,
/// and collapse `\` to `/` so Windows-spelled paths compare equal to POSIX ones.
fn normalize_member_path(p: &str) -> String {
    let p = p.trim().replace('\\', "/");
    let p = p.strip_prefix("./").unwrap_or(&p);
    p.trim_end_matches('/').to_string()
}

/// True for D-JPK-REF1 bare local-path refs.
pub fn is_bare_path(raw: &str) -> bool {
    raw.starts_with("./") || raw.starts_with("../") || raw.starts_with('/')
}

/// Attach canonical built-in package source to a bare package ref. Workspace
/// member/path refs stay untouched and are resolved by the caller's index.
pub fn with_default_source(raw: &str) -> String {
    let raw = raw.trim();
    if raw.contains(Syntax::REF_PROVIDER_AT) || is_bare_path(raw) {
        return raw.to_string();
    }
    match raw.split_once(Syntax::REF_CHANNEL_MARKER) {
        Some((package, selector)) if !package.is_empty() && !selector.is_empty() => format!(
            "{package}{at}{source}{marker}{selector}",
            at = Syntax::REF_PROVIDER_AT,
            source = Syntax::REF_SOURCE_JETPACK,
            marker = Syntax::REF_CHANNEL_MARKER,
        ),
        _ => format!(
            "{raw}{at}{source}",
            at = Syntax::REF_PROVIDER_AT,
            source = Syntax::REF_SOURCE_JETPACK,
        ),
    }
}

fn provider_first(provider: &str, target: &str, raw: &str) -> RefError {
    let replacement = if provider == Syntax::REF_SOURCE_PATH {
        target.to_string()
    } else {
        format!("{target}{}{provider}", Syntax::REF_PROVIDER_AT)
    };
    RefError::ProviderFirst {
        raw: raw.to_string(),
        replacement,
    }
}

/// Classify a `name['#'selector]['@'source]` ref against built-in sources.
/// This is the strict path for direct CLI refs. A declared monorepo source also
/// accepts D-MONOREF1's `source.package` form.
pub fn classify(raw: &str) -> Result<RefSpec, RefError> {
    classify_in(raw, &SourceTable::empty())
}

/// Classify a ref, accepting built-in sources plus any named source declared in
/// `table` (D-JPK17). D-JPK-REF1=A puts the package before `@`; a local path is
/// bare. A provider word before `@` is a teaching error only when the suffix is
/// not a source; a valid source suffix makes the left half a package name.
pub fn classify_in(raw: &str, table: &SourceTable) -> Result<RefSpec, RefError> {
    let raw = raw.trim();

    if let Some(error) = noncanonical_input_error(raw) {
        return Err(error);
    }

    if is_bare_path(raw) && !raw.contains(Syntax::REF_PROVIDER_AT) {
        return Ok(RefSpec {
            source: Source::Path,
            package: raw.to_string(),
            raw: raw.to_string(),
        });
    }

    if let Some((package, source_with_selector)) = raw.rsplit_once(Syntax::REF_PROVIDER_AT) {
        if source_with_selector.is_empty() || package.is_empty() {
            return Err(RefError::EmptyHalf(raw.to_string()));
        }
        let (source, selector) = match source_with_selector
            .split_once(Syntax::REF_CHANNEL_MARKER)
        {
            Some((source, selector)) if !source.is_empty() && !selector.is_empty() => {
                (source, Some(selector))
            }
            Some(_) => return Err(RefError::EmptyHalf(raw.to_string())),
            None => (source_with_selector, None),
        };
        let src = match Source::builtin(source) {
            Some(Source::Path) => {
                return Err(RefError::PathProviderRetired {
                    raw: raw.to_string(),
                    path: package.to_string(),
                })
            }
            Some(b) => b,
            None if table.upstream(source).is_some() => Source::Named(source.to_string()),
            None if Source::is_builtin(package) => {
                return Err(provider_first(package, source, raw))
            }
            None => {
                return Err(RefError::UnknownSource {
                    source: source.to_string(),
                    raw: raw.to_string(),
                    declared: table.declared_names(),
                })
            }
        };
        let package = selector.map_or_else(
            || package.to_string(),
            |selector| format!("{package}{}{selector}", Syntax::REF_CHANNEL_MARKER),
        );
        return Ok(RefSpec {
            source: src,
            package,
            raw: raw.to_string(),
        });
    }

    if let Some((source, package)) = raw.split_once('.') {
        if !source.is_empty() && !package.is_empty() && table.upstream(source).is_some() {
            return Ok(RefSpec {
                source: Source::Named(source.to_string()),
                package: package.to_string(),
                raw: raw.to_string(),
            });
        }
    }

    // D-JPK-SNIXREUSE1=A: a bare package is the Jetpack catalog spelling.
    // Workspace-aware callers probe their member index before reaching this
    // branch; a direct classifier has no workspace to consult.
    if !raw.is_empty() && !raw.contains('/') {
        return classify_in(&with_default_source(raw), table);
    }

    Err(RefError::MissingSeparator(raw.to_string()))
}

/// Classify a ref with workspace-member awareness (Slice B, D-MONOREF1=A).
///
/// Resolution order, first match wins:
///   1. source forms `package@source` and `source.package` — via `classify_in`
///   2. local form  `./package` — via `classify_in`
///   3. path form   `infra/logging` — exact relative-path match in the index
///   4. bare form   `logging` — exact member-name match in the index
///
/// The source form is tried first so an explicit source suffix always wins
/// over an accidental index collision. Only a ref with no source suffix (what
/// `classify_in` reports as `MissingSeparator`) falls through to the index. A
/// path/bare ref that matches no member is `UnknownMember` (E1231); one that
/// matches more than one is `AmbiguousMember` (E1230).
pub fn classify_with_workspace(
    raw: &str,
    table: &SourceTable,
    index: &WorkspaceIndex,
) -> Result<RefSpec, RefError> {
    let raw = raw.trim();
    if !raw.contains(Syntax::REF_PROVIDER_AT) && !is_bare_path(raw) && !index.is_empty() {
        match resolve_in_index(raw, index) {
            Ok(spec) => return Ok(spec),
            Err(RefError::AmbiguousMember { .. }) => {
                return resolve_in_index(raw, index);
            }
            Err(RefError::UnknownMember { .. }) => {}
            Err(other) => return Err(other),
        }
    }
    match classify_in(raw, table) {
        Ok(spec) => Ok(spec),
        // No source suffix. Consult the workspace index only when a workspace
        // actually exists — outside a monorepo a bare/path ref is still just a
        // missing-source error (D-JPK7), not an unknown-member error.
        Err(RefError::MissingSeparator(raw_owned)) => {
            if index.is_empty() {
                Err(RefError::MissingSeparator(raw_owned))
            } else {
                resolve_in_index(raw, index)
            }
        }
        Err(other) => Err(other),
    }
}

/// Resolve a source-prefix-less ref against the workspace index. `raw` with a
/// `/` is path form; otherwise bare form.
fn resolve_in_index(raw: &str, index: &WorkspaceIndex) -> Result<RefSpec, RefError> {
    let raw = raw.trim();
    let query = raw;
    let matches = if raw.contains('/') {
        index.by_path(query)
    } else {
        index.by_name(query)
    };
    match matches.len() {
        1 => Ok(RefSpec {
            // A workspace member is a local package directory: it realizes
            // through the `path` source at its relative path.
            source: Source::Path,
            package: matches[0].to_string(),
            raw: raw.to_string(),
        }),
        0 => Err(RefError::UnknownMember {
            query: raw.to_string(),
            suggestions: nearest(query, &index.candidate_labels()),
        }),
        _ => Err(RefError::AmbiguousMember {
            query: raw.to_string(),
            candidates: matches.iter().map(|s| s.to_string()).collect(),
        }),
    }
}

/// Up to three closest labels to `query` by a cheap prefix/substring/edit
/// heuristic, for a did-you-mean list. Deterministic (stable sort).
fn nearest(query: &str, labels: &[String]) -> Vec<String> {
    let mut scored: Vec<(usize, &String)> = labels
        .iter()
        .map(|l| (edit_distance(query, l), l))
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)));
    scored
        .into_iter()
        .filter(|(d, l)| *d <= query.len().max(l.len()) / 2 + 1)
        .take(3)
        .map(|(_, l)| l.clone())
        .collect()
}

/// Classic Levenshtein distance (std-only, I6). Small inputs (ref names), so the
/// simple two-row DP is more than fast enough.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

// ──────────────────────────────────────────────
// `target@provider` source refs (D-JPK-REF1=A; amends U6).
//
// The typed authoring surface (env.jet/package.jet `sources:`/`packages:`) writes source
// refs as `target@provider` — `owner/repo/rev@github`, `channel@nixpkgs`.
// Local paths are bare (`./local`, `../local`, `/opt/local`).
//
// This is the foundational classifier (JPK-0 / Chunk 1). Pack-file parsing and
// the user-facing diagnostics that render these errors land with the manifest
// reshape + module surface chunks; until then the typed `RefError` is internal.
// ──────────────────────────────────────────────

/// A classified `target@provider` source ref. `provider` is a built-in source
/// (github / nixpkgs, or path for a bare path); `target` is the upstream locator
/// understands (a repo+rev, a local path, a channel/pin).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRef {
    pub provider: Source,
    pub target: String,
    pub channel: Option<ChannelRef>,
    pub policy: ChannelPolicy,
    pub raw: String,
}

impl ProviderRef {
    /// Reconstruct the provider-first upstream form without dropping a
    /// channel selector. The source table uses this form for lock/update
    /// resolution, so channel intent must survive the typed lowering pass.
    pub fn upstream(&self) -> String {
        let channel = self
            .channel
            .as_ref()
            .map(|channel| format!("{}{}", Syntax::REF_CHANNEL_MARKER, channel.as_str()))
            .unwrap_or_default();
        format!(
            "{}{}{}{}",
            self.provider.label(),
            Syntax::REF_SEPARATOR,
            self.target,
            channel
        )
    }
}

/// D-JPK-CHANNEL1=A: a source ref tracking intent, resolved only by update
/// verbs and stored exact in `.jet/lock`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelRef {
    Latest,
    Main,
    SemverMask(String),
}

impl ChannelRef {
    pub fn parse_selector(selector: &str) -> Option<ChannelRef> {
        match selector {
            Syntax::REF_CHANNEL_LATEST => Some(ChannelRef::Latest),
            Syntax::REF_CHANNEL_MAIN => Some(ChannelRef::Main),
            s if is_semver_mask(s) => Some(ChannelRef::SemverMask(s.to_string())),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            ChannelRef::Latest => Syntax::REF_CHANNEL_LATEST,
            ChannelRef::Main => Syntax::REF_CHANNEL_MAIN,
            ChannelRef::SemverMask(s) => s,
        }
    }
}

fn is_semver_mask(s: &str) -> bool {
    let Some(rest) = s.strip_prefix('v') else {
        return false;
    };
    let Some(series) = rest.strip_suffix(".x") else {
        return false;
    };
    !series.is_empty() && series.chars().all(|c| c.is_ascii_digit())
}

/// The canonical form of a ref read from a persisted manifest or lock.
///
/// This is the one disk migration seam. User input never calls it to accept a
/// retired spelling; the classifiers compare its result with the input and
/// report the canonical replacement instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigratedRef {
    pub canonical: String,
    pub policy: ChannelPolicy,
}

fn legacy_policy_prefix(s: &str) -> (Option<ChannelPolicy>, &str) {
    for (marker, policy) in [
        (Syntax::REF_CHANNEL_AUTO, ChannelPolicy::Automatic),
        (Syntax::REF_CHANNEL_LATEST, ChannelPolicy::Manual),
    ] {
        let prefix = format!("{}{marker}", Syntax::REF_CHANNEL_MARKER);
        if let Some(rest) = s.strip_prefix(&prefix) {
            if rest.chars().next().is_some_and(char::is_whitespace) {
                return (Some(policy), rest.trim_start());
            }
        }
    }
    (None, s)
}

fn policy_for_selector(selector: &str) -> Option<ChannelPolicy> {
    match selector {
        Syntax::REF_CHANNEL_AUTO => Some(ChannelPolicy::Automatic),
        Syntax::REF_CHANNEL_LATEST => Some(ChannelPolicy::Manual),
        _ if ChannelRef::parse_selector(selector).is_some() => Some(ChannelPolicy::Manual),
        _ => None,
    }
}

fn valid_source_token(source: &str) -> bool {
    !source.is_empty()
        && source
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// Migrate one persisted ref from the retired selector positions.
pub fn migrate_persisted_ref(raw: &str) -> MigratedRef {
    let raw = raw.trim();
    let (legacy_policy, raw_ref) = legacy_policy_prefix(raw);
    let Some((left, source_with_selector)) = raw_ref.rsplit_once(Syntax::REF_PROVIDER_AT) else {
        return MigratedRef {
            canonical: raw.to_string(),
            policy: legacy_policy.unwrap_or_default(),
        };
    };
    if left.is_empty() || source_with_selector.is_empty() {
        return MigratedRef {
            canonical: raw.to_string(),
            policy: legacy_policy.unwrap_or_default(),
        };
    }
    let (source, source_selector) = match source_with_selector.split_once(Syntax::REF_CHANNEL_MARKER)
    {
        Some((source, selector)) if valid_source_token(source) && !selector.is_empty() => {
            (source, Some(selector))
        }
        Some(_) => {
            return MigratedRef {
                canonical: raw.to_string(),
                policy: legacy_policy.unwrap_or_default(),
            }
        }
        None if valid_source_token(source_with_selector) => (source_with_selector, None),
        None => {
            return MigratedRef {
                canonical: raw.to_string(),
                policy: legacy_policy.unwrap_or_default(),
            }
        }
    };
    let (package, target_selector) = match left.rsplit_once(Syntax::REF_CHANNEL_MARKER) {
        Some((package, selector)) if !package.is_empty() && !selector.is_empty() => {
            (package, Some(selector))
        }
        Some(_) => {
            return MigratedRef {
                canonical: raw.to_string(),
                policy: legacy_policy.unwrap_or_default(),
            }
        }
        None => (left, None),
    };
    let selector = source_selector.or(target_selector).or_else(|| {
        legacy_policy.map(|policy| match policy {
            ChannelPolicy::Automatic => Syntax::REF_CHANNEL_AUTO,
            ChannelPolicy::Manual => Syntax::REF_CHANNEL_LATEST,
            ChannelPolicy::Pinned => "",
        })
    });
    let canonical = match selector {
        Some(selector) if !selector.is_empty() => format!(
            "{package}{}{source}{}{selector}",
            Syntax::REF_PROVIDER_AT,
            Syntax::REF_CHANNEL_MARKER
        ),
        _ => format!("{package}{}{source}", Syntax::REF_PROVIDER_AT),
    };
    MigratedRef {
        canonical,
        policy: legacy_policy
            .or_else(|| selector.and_then(policy_for_selector))
            .unwrap_or_default(),
    }
}

/// Migrate public package/source input without changing the persisted spelling.
///
/// `nixpkgs` is retained by [`migrate_persisted_ref`] so locks and receipts can
/// show the upstream provenance. Public refs get the exact `@jetpack` spelling
/// instead; classifiers use the difference between these two migrations to
/// issue the teaching diagnostic.
pub fn migrate_public_ref(raw: &str) -> MigratedRef {
    let persisted = migrate_persisted_ref(raw);
    let canonical = rewrite_public_nixpkgs(&persisted.canonical)
        .unwrap_or_else(|| persisted.canonical.clone());
    MigratedRef {
        canonical,
        policy: persisted.policy,
    }
}

/// Return the user-facing error for a retired source spelling without routing
/// canonical input through the persisted-data migrator.
fn noncanonical_input_error(raw: &str) -> Option<RefError> {
    let retired = retired_selector_replacement(raw);
    let canonical = retired.as_deref().unwrap_or(raw);
    if let Some(replacement) = rewrite_public_nixpkgs(canonical) {
        return Some(RefError::RetiredNixpkgs {
            raw: raw.to_string(),
            replacement,
        });
    }
    retired.map(|replacement| RefError::NonCanonical {
        raw: raw.to_string(),
        replacement,
    })
}

/// Rewrite only the deleted selector positions for a diagnostic. This is not
/// a parser and is never used to classify or accept user input.
fn retired_selector_replacement(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let (legacy_policy, raw_ref) = legacy_policy_prefix(raw);
    let (left, source_with_selector) = raw_ref.rsplit_once(Syntax::REF_PROVIDER_AT)?;
    if left.is_empty() || source_with_selector.is_empty() {
        return None;
    }
    let (source, source_selector) = source_with_selector
        .split_once(Syntax::REF_CHANNEL_MARKER)
        .map_or((source_with_selector, None), |(source, selector)| {
            (source, Some(selector))
        });
    let (package, target_selector) = left
        .rsplit_once(Syntax::REF_CHANNEL_MARKER)
        .map_or((left, None), |(package, selector)| {
            (package, Some(selector))
        });
    if target_selector.is_none() && legacy_policy.is_none() {
        return None;
    }
    let selector = source_selector.or(target_selector).or_else(|| {
        legacy_policy.map(|policy| match policy {
            ChannelPolicy::Automatic => Syntax::REF_CHANNEL_AUTO,
            ChannelPolicy::Manual => Syntax::REF_CHANNEL_LATEST,
            ChannelPolicy::Pinned => "",
        })
    })?;
    if package.is_empty() || selector.is_empty() {
        return None;
    }
    Some(format!(
        "{package}{}{source}{}{selector}",
        Syntax::REF_PROVIDER_AT,
        Syntax::REF_CHANNEL_MARKER
    ))
}

fn rewrite_public_nixpkgs(raw: &str) -> Option<String> {
    let (package, source_with_selector) = raw.rsplit_once(Syntax::REF_PROVIDER_AT)?;
    if package.is_empty() || source_with_selector.is_empty() {
        return None;
    }
    let (source, selector) = source_with_selector
        .split_once(Syntax::REF_CHANNEL_MARKER)
        .map_or((source_with_selector, None), |(source, selector)| {
            (source, Some(selector))
        });
    if source != Syntax::REF_SOURCE_NIXPKGS || selector.is_some_and(str::is_empty) {
        return None;
    }
    Some(match selector {
        Some(selector) => format!(
            "{package}{}{jetpack}{}{selector}",
            Syntax::REF_PROVIDER_AT,
            Syntax::REF_CHANNEL_MARKER,
            jetpack = Syntax::REF_SOURCE_JETPACK,
        ),
        None => format!(
            "{package}{}{jetpack}",
            Syntax::REF_PROVIDER_AT,
            jetpack = Syntax::REF_SOURCE_JETPACK,
        ),
    })
}

/// Canonical ref used in lock and receipt identity. Public nixpkgs spelling
/// remains classifiable during migration, but cannot create a second lock key.
pub fn canonical_locked_ref(raw: &str) -> String {
    let canonical = migrate_persisted_ref(raw).canonical;
    let Some((package, source_with_selector)) = canonical.rsplit_once(Syntax::REF_PROVIDER_AT)
    else {
        return canonical;
    };
    let (source, selector) = source_with_selector
        .split_once(Syntax::REF_CHANNEL_MARKER)
        .map_or((source_with_selector, None), |(source, selector)| {
            (source, Some(selector))
        });
    if source != Syntax::REF_SOURCE_NIXPKGS && source != Syntax::REF_SOURCE_JETPACK {
        return canonical;
    }
    let suffix = selector
        .map(|selector| format!("{}{}", Syntax::REF_CHANNEL_MARKER, selector))
        .unwrap_or_default();
    format!(
        "{package}{at}{source}{suffix}",
        at = Syntax::REF_PROVIDER_AT,
        source = Syntax::REF_SOURCE_JETPACK,
    )
}

/// Return policy encoded in a canonical user ref. Retired prefix input is not
/// accepted here and therefore returns the pinned default.
pub fn policy_for_ref(raw: &str) -> ChannelPolicy {
    raw.trim()
        .rsplit_once(Syntax::REF_PROVIDER_AT)
        .and_then(|(_, source)| source.rsplit_once(Syntax::REF_CHANNEL_MARKER))
        .and_then(|(_, selector)| policy_for_selector(selector))
        .unwrap_or_default()
}

/// Split an internal provider-first target at `#` when that selector is a channel.
/// Exact selectors such as `#v1.2.3` are left untouched.
pub fn split_channel_ref(s: &str) -> (&str, Option<ChannelRef>) {
    match s.rsplit_once('#') {
        Some((base, selector)) => match ChannelRef::parse_selector(selector) {
            Some(ch) => (base, Some(ch)),
            None => (s, None),
        },
        None => (s, None),
    }
}

fn policy_for_upstream(upstream: &str) -> ChannelPolicy {
    split_channel_ref(upstream)
        .1
        .map_or(ChannelPolicy::Pinned, |_| ChannelPolicy::Manual)
}

/// Classify a `target@provider[#selector]` source ref or a bare local path.
pub fn classify_provider_ref(raw: &str) -> Result<ProviderRef, RefError> {
    let raw = raw.trim();
    if let Some(error) = noncanonical_input_error(raw) {
        return Err(error);
    }
    if is_bare_path(raw) && !raw.contains(Syntax::REF_PROVIDER_AT) {
        return Ok(ProviderRef {
            provider: Source::Path,
            target: raw.to_string(),
            channel: None,
            policy: ChannelPolicy::Pinned,
            raw: raw.to_string(),
        });
    }
    let (target, provider_with_selector) = match raw.rsplit_once(Syntax::REF_PROVIDER_AT) {
        Some(parts) => parts,
        None => return Err(RefError::MissingSeparator(raw.to_string())),
    };
    if provider_with_selector.is_empty() || target.is_empty() {
        return Err(RefError::EmptyHalf(raw.to_string()));
    }
    if Source::is_builtin(target) {
        return Err(provider_first(target, provider_with_selector, raw));
    }
    let (provider, selector) = match provider_with_selector.split_once(Syntax::REF_CHANNEL_MARKER) {
        Some((provider, selector)) if !provider.is_empty() && !selector.is_empty() => {
            (provider, Some(selector))
        }
        Some(_) => return Err(RefError::EmptyHalf(raw.to_string())),
        None => (provider_with_selector, None),
    };
    let provider = match Source::builtin(provider) {
        Some(Source::Path) => {
            return Err(RefError::PathProviderRetired {
                raw: raw.to_string(),
                path: target.to_string(),
            })
        }
        Some(b) => b,
        None => {
            return Err(RefError::UnknownSource {
                source: provider.to_string(),
                raw: raw.to_string(),
                declared: Vec::new(),
            })
        }
    };
    let (target, channel, policy) = match selector {
        Some(Syntax::REF_CHANNEL_AUTO) => (
            target.to_string(),
            Some(ChannelRef::Latest),
            ChannelPolicy::Automatic,
        ),
        Some(Syntax::REF_CHANNEL_LATEST) => (
            target.to_string(),
            Some(ChannelRef::Latest),
            ChannelPolicy::Manual,
        ),
        Some(selector) => match ChannelRef::parse_selector(selector) {
            Some(channel) => (target.to_string(), Some(channel), ChannelPolicy::Manual),
            None => (
                format!("{target}{}{selector}", Syntax::REF_CHANNEL_MARKER),
                None,
                ChannelPolicy::Pinned,
            ),
        },
        None => (target.to_string(), None, ChannelPolicy::Pinned),
    };
    Ok(ProviderRef {
        provider,
        target,
        channel,
        policy,
        raw: raw.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retires_nixpkgs_public_spelling_but_preserves_persisted_provenance() {
        for (raw, replacement, persisted) in [
            (
                "ripgrep@nixpkgs",
                "ripgrep@jetpack",
                "ripgrep@nixpkgs",
            ),
            (
                "ripgrep@nixpkgs#auto",
                "ripgrep@jetpack#auto",
                "ripgrep@nixpkgs#auto",
            ),
            (
                "ripgrep#version=15.2.0@nixpkgs",
                "ripgrep@jetpack#version=15.2.0",
                "ripgrep@nixpkgs#version=15.2.0",
            ),
        ] {
            assert_eq!(migrate_public_ref(raw).canonical, replacement);
            assert_eq!(migrate_persisted_ref(raw).canonical, persisted);
            assert_eq!(
                classify(raw),
                Err(RefError::RetiredNixpkgs {
                    raw: raw.into(),
                    replacement: replacement.into(),
                })
            );
        }
    }

    #[test]
    fn classifies_jetpack_and_canonicalizes_bare_package_refs() {
        let explicit = classify("ripgrep@jetpack").unwrap();
        assert_eq!(explicit.source, Source::Jetpack);
        assert_eq!(with_default_source("ripgrep"), "ripgrep@jetpack");
        assert_eq!(
            canonical_locked_ref(&with_default_source("ripgrep")),
            canonical_locked_ref("ripgrep@jetpack")
        );
        assert_eq!(canonical_locked_ref("ripgrep@nixpkgs"), "ripgrep@jetpack");
    }

    #[test]
    fn bare_and_explicit_jetpack_refs_classify_identically() {
        assert_eq!(
            classify("ripgrep").unwrap(),
            classify("ripgrep@jetpack").unwrap()
        );
    }

    #[test]
    fn nixpkgs_public_ref_is_an_exact_jetpack_rewrite() {
        assert!(matches!(
            classify("ripgrep@nixpkgs"),
            Err(RefError::RetiredNixpkgs { replacement, .. })
                if replacement == "ripgrep@jetpack"
        ));
    }

    #[test]
    fn classifies_github_repo() {
        let r = classify("halcyonomega/my-fastfetch-jet-config@github").unwrap();
        assert_eq!(r.source, Source::Github);
        assert_eq!(r.package, "halcyonomega/my-fastfetch-jet-config");
        assert_eq!(r.short_name(), "my-fastfetch-jet-config");
    }

    #[test]
    fn classifies_local_path() {
        let r = classify("./my-env").unwrap();
        assert_eq!(r.source, Source::Path);
        assert_eq!(r.package, "./my-env");
    }

    #[test]
    fn classifies_direct_cran_root_with_exact_version() {
        let r = classify("jsonlite@cran#version=1.9.0").unwrap();
        assert_eq!(r.source, Source::Cran);
        assert_eq!(r.package, "jsonlite#version=1.9.0");
    }

    #[test]
    fn classifies_direct_luarocks_root_with_exact_version() {
        let r = classify("luasocket@luarocks#version=3.1.0-1").unwrap();
        assert_eq!(r.source, Source::LuaRocks);
        assert_eq!(r.package, "luasocket#version=3.1.0-1");
        assert_eq!(ProviderKind::parse("luarocks"), ProviderKind::LuaRocks);
    }

    #[test]
    fn classifies_direct_scripting_registry_roots() {
        for (raw, source, provider) in [
            (
                "rack@ruby#version=3.2.0",
                Source::RubyGems,
                ProviderKind::RubyGems,
            ),
            (
                "JSON-MaybeXS@perl#version=1.004008",
                Source::Cpan,
                ProviderKind::Cpan,
            ),
            (
                "monolog/monolog@php#version=3.9.0",
                Source::Packagist,
                ProviderKind::Packagist,
            ),
        ] {
            let spec = classify(raw).unwrap();
            assert_eq!(spec.source, source);
            assert_eq!(ProviderKind::parse(spec.source.label()), provider);
        }
    }

    #[test]
    fn classifies_direct_jet_registry_npm_and_cargo_roots() {
        for (raw, source, provider) in [
            (
                "hello@jet-registry#version=1.0.0",
                Source::JetRegistry,
                ProviderKind::JetRegistry,
            ),
            ("left-pad@npm#version=1.3.0", Source::Npm, ProviderKind::Npm),
            (
                "serde@cargo#version=1.0.200",
                Source::Cargo,
                ProviderKind::Cargo,
            ),
        ] {
            let spec = classify(raw).unwrap();
            assert_eq!(spec.source, source);
            assert_eq!(ProviderKind::parse(spec.source.label()), provider);
        }
    }

    #[test]
    fn classifies_native_release_source() {
        let spec = classify("omp@releases").unwrap();
        assert_eq!(spec.source, Source::Releases);
        assert_eq!(spec.package, "omp");
        assert_eq!(ProviderKind::parse("jetpackage"), ProviderKind::JetPackage);
    }

    #[test]
    fn short_name_discards_version_selector() {
        let spec = classify("omp@releases#18.0.0").unwrap();
        assert_eq!(spec.short_name(), "omp");
    }

    #[test]
    fn rejects_missing_separator() {
        assert_eq!(
            classify("fastfetch"),
            Err(RefError::MissingSeparator("fastfetch".into()))
        );
    }

    #[test]
    fn rejects_empty_halves() {
        assert!(matches!(classify("@nixpkgs"), Err(RefError::EmptyHalf(_))));
        assert!(matches!(
            classify("fastfetch@"),
            Err(RefError::EmptyHalf(_))
        ));
    }

    #[test]
    fn selector_without_source_still_needs_resolution() {
        assert!(matches!(
            classify("fastfetch#latest"),
            Err(RefError::MissingSeparator(_))
        ));
    }

    #[test]
    fn rejects_unknown_source() {
        match classify("wget@brew") {
            Err(RefError::UnknownSource { source, .. }) => assert_eq!(source, "brew"),
            other => panic!("expected UnknownSource, got {other:?}"),
        }
    }

    #[test]
    fn old_provider_first_ref_teaches_the_flip() {
        assert_eq!(
            classify("github@owner/repo"),
            Err(RefError::ProviderFirst {
                raw: "github@owner/repo".into(),
                replacement: "owner/repo@github".into(),
            })
        );
        assert_eq!(
            classify("path@../helpers"),
            Err(RefError::ProviderFirst {
                raw: "path@../helpers".into(),
                replacement: "../helpers".into(),
            })
        );
    }

    #[test]
    fn package_ref_keeps_version_after_source() {
        let table = SourceTable::from_decls([(
            "vendor".to_string(),
            "acme/helpers@github".to_string(),
            ProviderKind::Core,
        )]);
        let r = classify_in("textkit@vendor#1.2.0", &table).unwrap();
        assert_eq!(r.package, "textkit#1.2.0");
        assert_eq!(r.source, Source::Named("vendor".into()));
    }

    #[test]
    fn provider_ref_marks_channel_selectors() {
        assert_eq!(
            classify_provider_ref("openai/codex@github#latest")
                .unwrap()
                .channel,
            Some(ChannelRef::Latest)
        );
        assert_eq!(
            classify_provider_ref("openai/codex@github#main")
                .unwrap()
                .channel,
            Some(ChannelRef::Main)
        );
        assert_eq!(
            classify_provider_ref("openai/codex@github#v0.x")
                .unwrap()
                .channel,
            Some(ChannelRef::SemverMask("v0.x".to_string()))
        );
        assert_eq!(
            classify_provider_ref("openai/codex@github#v0.50.1")
                .unwrap()
                .channel,
            None
        );
    }

    #[test]
    fn classifies_declared_named_source() {
        let table = SourceTable::from_decls([(
            "stable".to_string(),
            "github:NixOS/nixpkgs/nixos-24.05".to_string(),
            ProviderKind::Nix,
        )]);
        let r = classify_in("ripgrep@stable", &table).unwrap();
        assert_eq!(r.source, Source::Named("stable".to_string()));
        assert_eq!(r.package, "ripgrep");
        assert_eq!(r.source.label(), "stable");
    }

    #[test]
    fn named_source_unknown_lists_declared() {
        let table = SourceTable::from_decls([
            ("stable".to_string(), "u1".to_string(), ProviderKind::Nix),
            ("unstable".to_string(), "u2".to_string(), ProviderKind::Core),
        ]);
        match classify_in("neovim@beta", &table) {
            Err(RefError::UnknownSource { declared, .. }) => {
                assert_eq!(declared, vec!["stable", "unstable"]);
            }
            other => panic!("expected UnknownSource, got {other:?}"),
        }
    }

    #[test]
    fn builtins_resolve_without_declaration() {
        let table = SourceTable::empty();
        assert!(matches!(
            classify_in("fd@nixpkgs", &table),
            Err(RefError::RetiredNixpkgs { replacement, .. })
                if replacement == "fd@jetpack"
        ));
        assert!(Source::is_builtin("nixpkgs"));
        assert!(!Source::is_builtin("stable"));
    }

    // ── workspace-index addressing (Slice B, D-MONOREF1=A) ──

    fn ws_index() -> WorkspaceIndex {
        WorkspaceIndex::from_members([
            ("logging".to_string(), "./infra/logging".to_string()),
            ("ranker".to_string(), "packages/ranker".to_string()),
        ])
    }

    #[test]
    fn bare_form_resolves_unique_member() {
        let r = classify_with_workspace("logging", &SourceTable::empty(), &ws_index()).unwrap();
        assert_eq!(r.source, Source::Path);
        assert_eq!(r.package, "infra/logging");
    }

    #[test]
    fn path_form_resolves_member() {
        // Path form matches the normalized relative path (`./` trimmed).
        let r =
            classify_with_workspace("infra/logging", &SourceTable::empty(), &ws_index()).unwrap();
        assert_eq!(r.source, Source::Path);
        assert_eq!(r.package, "infra/logging");
        // The other member by its stored path.
        let r2 =
            classify_with_workspace("packages/ranker", &SourceTable::empty(), &ws_index()).unwrap();
        assert_eq!(r2.package, "packages/ranker");
    }

    #[test]
    fn unknown_member_is_e1231_with_suggestion() {
        let err = classify_with_workspace("loggin", &SourceTable::empty(), &ws_index())
            .expect_err("typo must not resolve");
        assert_eq!(err.code(), Some("E1231"));
        match err {
            RefError::UnknownMember { suggestions, .. } => {
                assert!(
                    suggestions.contains(&"logging".to_string()),
                    "expected a did-you-mean for `logging`, got {suggestions:?}"
                );
            }
            other => panic!("expected UnknownMember, got {other:?}"),
        }
    }

    #[test]
    fn ambiguous_bare_member_is_e1230_lists_candidates() {
        let index = WorkspaceIndex::from_members([
            ("logging".to_string(), "infra/logging".to_string()),
            ("logging".to_string(), "apps/logging".to_string()),
        ]);
        let err = classify_with_workspace("logging", &SourceTable::empty(), &index)
            .expect_err("two members share the name — must be ambiguous");
        assert_eq!(err.code(), Some("E1230"));
        match err {
            RefError::AmbiguousMember { candidates, .. } => {
                assert_eq!(candidates.len(), 2);
                assert!(candidates.contains(&"infra/logging".to_string()));
                assert!(candidates.contains(&"apps/logging".to_string()));
            }
            other => panic!("expected AmbiguousMember, got {other:?}"),
        }
    }

    #[test]
    fn source_form_wins_over_index() {
        // An explicit `package@source` is never shadowed by the index.
        let table =
            SourceTable::from_decls([("nixpkgs".to_string(), "u".to_string(), ProviderKind::Nix)]);
        let r = classify_with_workspace("logging@jetpack", &table, &ws_index()).unwrap();
        assert_eq!(r.source, Source::Jetpack);
        assert_eq!(r.package, "logging");
    }

    #[test]
    fn empty_index_falls_through_to_missing_separator() {
        // With no workspace, a bare ref is still a plain missing-source error —
        // member resolution is a monorepo-only feature, never a surprise when
        // there is no workspace at all.
        let err =
            classify_with_workspace("logging", &SourceTable::empty(), &WorkspaceIndex::empty())
                .expect_err("no workspace → not a member");
        assert!(
            matches!(err, RefError::MissingSeparator(_)),
            "expected MissingSeparator, got {err:?}"
        );
    }

    // ── target@provider source refs (D-JPK-REF1=A; amends U6) ──

    #[test]
    fn provider_ref_github_with_rev() {
        let r = classify_provider_ref("NixOS/nixpkgs/nixos-24.05@github").unwrap();
        assert_eq!(r.provider, Source::Github);
        assert_eq!(r.target, "NixOS/nixpkgs/nixos-24.05");
    }

    #[test]
    fn provider_ref_local_path() {
        let r = classify_provider_ref("../helpers").unwrap();
        assert_eq!(r.provider, Source::Path);
        assert_eq!(r.target, "../helpers");
    }

    #[test]
    fn provider_ref_retires_nixpkgs_source_spelling() {
        assert_eq!(
            classify_provider_ref("nixpkgs-unstable@nixpkgs"),
            Err(RefError::RetiredNixpkgs {
                raw: "nixpkgs-unstable@nixpkgs".into(),
                replacement: "nixpkgs-unstable@jetpack".into(),
            })
        );
    }

    #[test]
    fn channel_policies_are_checked_and_keep_channel_intent() {
        let pinned = classify_provider_ref("rustc@jetpack").unwrap();
        assert_eq!(pinned.policy, ChannelPolicy::Pinned);
        assert_eq!(pinned.channel, None);

        let manual = classify_provider_ref("jq@jetpack#latest").unwrap();
        assert_eq!(manual.policy, ChannelPolicy::Manual);
        assert_eq!(manual.target, "jq");
        assert_eq!(manual.channel, Some(ChannelRef::Latest));

        let automatic = classify_provider_ref("omp@jetpack#auto").unwrap();
        assert_eq!(automatic.policy, ChannelPolicy::Automatic);
        assert_eq!(automatic.target, "omp");
        assert_eq!(automatic.channel, Some(ChannelRef::Latest));

        assert!(matches!(
            classify_provider_ref(&format!("omp#auto{}jetpack", Syntax::REF_PROVIDER_AT)),
            Err(RefError::NonCanonical { replacement, .. })
                if replacement == "omp@jetpack#auto"
        ));
    }

    #[test]
    fn retired_selector_forms_do_not_fall_back_to_canonical_parsers() {
        for (raw, replacement) in [
            ("#auto omp@releases", "omp@releases#auto"),
            ("omp#v18.0.4@releases", "omp@releases#v18.0.4"),
        ] {
            assert!(matches!(
                classify(raw),
                Err(RefError::NonCanonical { replacement: actual, .. })
                    if actual == replacement
            ));
            assert!(matches!(
                classify_provider_ref(raw),
                Err(RefError::NonCanonical { replacement: actual, .. })
                    if actual == replacement
            ));
        }
    }

    #[test]
    fn provider_ref_splits_on_last_at() {
        let r = classify_provider_ref("owner@host/repo@github").unwrap();
        assert_eq!(r.provider, Source::Github);
        assert_eq!(r.target, "owner@host/repo");
    }

    #[test]
    fn provider_ref_rejects_missing_at() {
        assert!(matches!(
            classify_provider_ref("github/NixOS/nixpkgs"),
            Err(RefError::MissingSeparator(_))
        ));
    }

    #[test]
    fn provider_ref_rejects_empty_halves() {
        assert!(matches!(
            classify_provider_ref("@github"),
            Err(RefError::EmptyHalf(_))
        ));
        assert!(matches!(
            classify_provider_ref("owner/repo@"),
            Err(RefError::EmptyHalf(_))
        ));
    }

    #[test]
    fn provider_ref_rejects_unknown_provider() {
        match classify_provider_ref("owner/repo@gitlab") {
            Err(RefError::UnknownSource { source, .. }) => assert_eq!(source, "gitlab"),
            other => panic!("expected UnknownSource, got {other:?}"),
        }
    }

    #[test]
    fn provider_ref_rejects_provider_first_and_path_word() {
        assert_eq!(
            classify_provider_ref("github@owner/repo"),
            Err(RefError::ProviderFirst {
                raw: "github@owner/repo".into(),
                replacement: "owner/repo@github".into(),
            })
        );
        assert_eq!(
            classify_provider_ref("../helpers@path"),
            Err(RefError::PathProviderRetired {
                raw: "../helpers@path".into(),
                path: "../helpers".into(),
            })
        );
    }
}
