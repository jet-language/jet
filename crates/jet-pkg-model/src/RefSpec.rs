//! Jetpack ref classifier: `name['#'selector]['@'source]` (D-JPK-REF1=A).
//!
//! A ref reads like an address: the package first, then its source. `#` pins a
//! version or channel. Bare `./`, `../`, and `/` paths need no provider word.
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
    /// The nixpkgs collection, realized through the Nix provider.
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
    /// A pack-declared named source, e.g. `stable` → a pinned nixpkgs (D-JPK17).
    Named(String),
}

impl Source {
    /// The source token as written after the `@` in a ref.
    pub fn label(&self) -> &str {
        match self {
            Source::Nixpkgs => Syntax::REF_SOURCE_NIXPKGS,
            Source::Github => Syntax::REF_SOURCE_GITHUB,
            Source::Path => Syntax::REF_SOURCE_PATH,
            Source::Cran => Syntax::REF_SOURCE_CRAN,
            Source::LuaRocks => Syntax::REF_SOURCE_LUAROCKS,
            Source::RubyGems => Syntax::REF_SOURCE_RUBY,
            Source::Cpan => Syntax::REF_SOURCE_PERL,
            Source::Packagist => Syntax::REF_SOURCE_PHP,
            Source::Named(name) => name,
        }
    }

    /// Whether `name` is one of the built-in source keywords.
    pub fn is_builtin(name: &str) -> bool {
        name == Syntax::REF_SOURCE_NIXPKGS
            || name == Syntax::REF_SOURCE_GITHUB
            || name == Syntax::REF_SOURCE_PATH
            || name == Syntax::REF_SOURCE_CRAN
            || name == Syntax::REF_SOURCE_LUAROCKS
            || name == Syntax::REF_SOURCE_RUBY
            || name == Syntax::REF_SOURCE_PERL
            || name == Syntax::REF_SOURCE_PHP
    }

    fn builtin(name: &str) -> Option<Source> {
        match name {
            n if n == Syntax::REF_SOURCE_NIXPKGS => Some(Source::Nixpkgs),
            n if n == Syntax::REF_SOURCE_GITHUB => Some(Source::Github),
            n if n == Syntax::REF_SOURCE_PATH => Some(Source::Path),
            n if n == Syntax::REF_SOURCE_CRAN => Some(Source::Cran),
            n if n == Syntax::REF_SOURCE_LUAROCKS => Some(Source::LuaRocks),
            n if n == Syntax::REF_SOURCE_RUBY => Some(Source::RubyGems),
            n if n == Syntax::REF_SOURCE_PERL => Some(Source::Cpan),
            n if n == Syntax::REF_SOURCE_PHP => Some(Source::Packagist),
            _ => None,
        }
    }
}

/// Which backend realizes a source: the `nix` compatibility provider or the
/// first-party `core` provider (R2). Default is `nix` (R1 behavior).
///
/// `Infer` is a third, *unresolved* state used by the typed surface (U9): a
/// `…@github` source's kind can't be known during pure `evaluate_env`
/// evaluation — it depends on whether the remote repo carries a `pkg.jet`,
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
    /// Decide `Nix` vs `Core` at realize time by peeking the source's
    /// `pkg.jet` (U9). Only the typed `…@github` surface produces this.
    Infer,
}

impl ProviderKind {
    /// Parse a provider name from a source declaration's third argument.
    /// Anything other than `core` is the default `nix`.
    pub fn parse(s: &str) -> ProviderKind {
        match s {
            "core" => ProviderKind::Core,
            "cran" => ProviderKind::Cran,
            "luarocks" => ProviderKind::LuaRocks,
            "ruby" => ProviderKind::RubyGems,
            "perl" => ProviderKind::Cpan,
            "php" => ProviderKind::Packagist,
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
            // Never user-shown: resolved before any listing/diagnostic.
            ProviderKind::Infer => "infer",
        }
    }
}

#[derive(Debug, Clone)]
struct SourceEntry {
    upstream: String,
    via: ProviderKind,
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
    /// the table for direct CLI refs (`jetpack run fastfetch@nixpkgs`).
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
                .map(|(name, upstream, via)| (name, SourceEntry { upstream, via }))
                .collect(),
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

    /// Merge `other` into this table, filling in names that are not already
    /// declared here. `self` wins on conflict — inline declarations take
    /// priority over `jetpack.toml` fallbacks.
    pub fn merge_defaults(&mut self, other: SourceTable) {
        for (name, entry) in other.named {
            self.named.entry(name).or_insert(entry);
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
        self.package.rsplit('/').next().unwrap_or(&self.package)
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
    ProviderFirst {
        raw: String,
        replacement: String,
    },
    /// The `path` provider word retired; local paths are bare.
    PathProviderRetired {
        raw: String,
        path: String,
    },
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
            RefError::ProviderFirst { .. } | RefError::PathProviderRetired { .. } => {
                Some("E1317")
            }
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
/// This is the strict path for direct CLI refs.
pub fn classify(raw: &str) -> Result<RefSpec, RefError> {
    classify_in(raw, &SourceTable::empty())
}

/// Classify a ref, accepting built-in sources plus any named source declared in
/// `table` (D-JPK17). D-JPK-REF1=A puts the package before `@`; a local path is
/// bare. A provider word before `@` is a teaching error, not a valid package
/// name, because silently accepting `github@owner/repo` would hide the flip.
pub fn classify_in(raw: &str, table: &SourceTable) -> Result<RefSpec, RefError> {
    let raw = raw.trim();

    if is_bare_path(raw) && !raw.contains(Syntax::REF_PROVIDER_AT) {
        return Ok(RefSpec {
            source: Source::Path,
            package: raw.to_string(),
            raw: raw.to_string(),
        });
    }

    if let Some((package, source)) = raw.rsplit_once(Syntax::REF_PROVIDER_AT) {
        if source.is_empty() || package.is_empty() {
            return Err(RefError::EmptyHalf(raw.to_string()));
        }
        if Source::is_builtin(package) {
            return Err(provider_first(package, source, raw));
        }
        let src = match Source::builtin(source) {
            Some(Source::Path) => {
                return Err(RefError::PathProviderRetired {
                    raw: raw.to_string(),
                    path: package.to_string(),
                })
            }
            Some(b) => b,
            None if table.upstream(source).is_some() => Source::Named(source.to_string()),
            None => {
                return Err(RefError::UnknownSource {
                    source: source.to_string(),
                    raw: raw.to_string(),
                    declared: table.declared_names(),
                })
            }
        };
        return Ok(RefSpec {
            source: src,
            package: package.to_string(),
            raw: raw.to_string(),
        });
    }

    Err(RefError::MissingSeparator(raw.to_string()))
}

/// Classify a ref with workspace-member awareness (Slice B, D-MONOREF1=A).
///
/// Resolution order, first match wins:
///   1. source form `package@source` — via `classify_in`
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
// The typed authoring surface (env.jet/pkg.jet `sources:`/`packages:`) writes source
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
    pub raw: String,
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
            "latest" => Some(ChannelRef::Latest),
            "main" => Some(ChannelRef::Main),
            s if is_semver_mask(s) => Some(ChannelRef::SemverMask(s.to_string())),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            ChannelRef::Latest => "latest",
            ChannelRef::Main => "main",
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

/// Split an upstream/source target at `#` when that selector is a channel.
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

/// Classify a `target@provider` source ref or a bare local path.
pub fn classify_provider_ref(raw: &str) -> Result<ProviderRef, RefError> {
    let raw = raw.trim();
    if is_bare_path(raw) && !raw.contains(Syntax::REF_PROVIDER_AT) {
        return Ok(ProviderRef {
            provider: Source::Path,
            target: raw.to_string(),
            channel: None,
            raw: raw.to_string(),
        });
    }
    let (target, provider) = match raw.rsplit_once(Syntax::REF_PROVIDER_AT) {
        Some(parts) => parts,
        None => return Err(RefError::MissingSeparator(raw.to_string())),
    };
    if provider.is_empty() || target.is_empty() {
        return Err(RefError::EmptyHalf(raw.to_string()));
    }
    if Source::is_builtin(target) {
        return Err(provider_first(target, provider, raw));
    }
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
    let (_, channel) = split_channel_ref(target);
    Ok(ProviderRef {
        provider,
        target: target.to_string(),
        channel,
        raw: raw.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_nixpkgs() {
        let r = classify("fastfetch@nixpkgs").unwrap();
        assert_eq!(r.source, Source::Nixpkgs);
        assert_eq!(r.package, "fastfetch");
        assert_eq!(r.short_name(), "fastfetch");
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
        let r = classify("jsonlite#version=1.9.0@cran").unwrap();
        assert_eq!(r.source, Source::Cran);
        assert_eq!(r.package, "jsonlite#version=1.9.0");
    }

    #[test]
    fn classifies_direct_luarocks_root_with_exact_version() {
        let r = classify("luasocket#version=3.1.0-1@luarocks").unwrap();
        assert_eq!(r.source, Source::LuaRocks);
        assert_eq!(r.package, "luasocket#version=3.1.0-1");
        assert_eq!(ProviderKind::parse("luarocks"), ProviderKind::LuaRocks);
    }

    #[test]
    fn classifies_direct_scripting_registry_roots() {
        for (raw, source, provider) in [
            (
                "rack#version=3.2.0@ruby",
                Source::RubyGems,
                ProviderKind::RubyGems,
            ),
            (
                "JSON-MaybeXS#version=1.004008@perl",
                Source::Cpan,
                ProviderKind::Cpan,
            ),
            (
                "monolog/monolog#version=3.9.0@php",
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
    fn rejects_missing_separator() {
        assert_eq!(
            classify("fastfetch"),
            Err(RefError::MissingSeparator("fastfetch".into()))
        );
    }

    #[test]
    fn rejects_empty_halves() {
        assert!(matches!(
            classify("@nixpkgs"),
            Err(RefError::EmptyHalf(_))
        ));
        assert!(matches!(classify("fastfetch@"), Err(RefError::EmptyHalf(_))));
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
    fn package_ref_keeps_version_before_source() {
        let table = SourceTable::from_decls([(
            "vendor".to_string(),
            "acme/helpers@github".to_string(),
            ProviderKind::Core,
        )]);
        let r = classify_in("textkit#1.2.0@vendor", &table).unwrap();
        assert_eq!(r.package, "textkit#1.2.0");
        assert_eq!(r.source, Source::Named("vendor".into()));
    }

    #[test]
    fn provider_ref_marks_channel_selectors() {
        assert_eq!(
            classify_provider_ref("openai/codex#latest@github")
                .unwrap()
                .channel,
            Some(ChannelRef::Latest)
        );
        assert_eq!(
            classify_provider_ref("openai/codex#main@github")
                .unwrap()
                .channel,
            Some(ChannelRef::Main)
        );
        assert_eq!(
            classify_provider_ref("openai/codex#v0.x@github")
                .unwrap()
                .channel,
            Some(ChannelRef::SemverMask("v0.x".to_string()))
        );
        assert_eq!(
            classify_provider_ref("openai/codex#v0.50.1@github")
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
        assert!(classify_in("fd@nixpkgs", &table).is_ok());
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
        let r = classify_with_workspace("logging@nixpkgs", &table, &ws_index()).unwrap();
        assert_eq!(r.source, Source::Nixpkgs);
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
    fn provider_ref_nixpkgs_channel() {
        let r = classify_provider_ref("nixpkgs-unstable@nixpkgs").unwrap();
        assert_eq!(r.provider, Source::Nixpkgs);
        assert_eq!(r.target, "nixpkgs-unstable");
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
