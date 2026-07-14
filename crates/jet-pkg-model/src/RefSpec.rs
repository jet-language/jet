//! Jetpack ref classifier: `<source>:<package/path>` (D-JPK7/15).
//!
//! The only public package syntax in Jetpack. Users never type Nix's `#`
//! selector — Jetpack translates `:` into the provider's form internally.
//! Examples:
//!   `nixpkgs:fastfetch`
//!   `github:halcyonomega/my-fastfetch-jet-config`
//!   `path:./my-env`

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
    /// A pack-declared named source, e.g. `stable` → a pinned nixpkgs (D-JPK17).
    Named(String),
}

impl Source {
    /// The source token as written before the `:` in a ref.
    pub fn label(&self) -> &str {
        match self {
            Source::Nixpkgs => Syntax::REF_SOURCE_NIXPKGS,
            Source::Github => Syntax::REF_SOURCE_GITHUB,
            Source::Path => Syntax::REF_SOURCE_PATH,
            Source::Cran => Syntax::REF_SOURCE_CRAN,
            Source::Named(name) => name,
        }
    }

    /// Whether `name` is one of the built-in source keywords.
    pub fn is_builtin(name: &str) -> bool {
        name == Syntax::REF_SOURCE_NIXPKGS
            || name == Syntax::REF_SOURCE_GITHUB
            || name == Syntax::REF_SOURCE_PATH
            || name == Syntax::REF_SOURCE_CRAN
    }

    fn builtin(name: &str) -> Option<Source> {
        match name {
            n if n == Syntax::REF_SOURCE_NIXPKGS => Some(Source::Nixpkgs),
            n if n == Syntax::REF_SOURCE_GITHUB => Some(Source::Github),
            n if n == Syntax::REF_SOURCE_PATH => Some(Source::Path),
            n if n == Syntax::REF_SOURCE_CRAN => Some(Source::Cran),
            _ => None,
        }
    }
}

/// Which backend realizes a source: the `nix` compatibility provider or the
/// first-party `core` provider (R2). Default is `nix` (R1 behavior).
///
/// `Infer` is a third, *unresolved* state used by the typed surface (U9): a
/// `github@…` source's kind can't be known during pure `evaluate_env`
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
    /// Decide `Nix` vs `Core` at realize time by peeking the source's
    /// `pkg.jet` (U9). Only the typed `github@…` surface produces this.
    Infer,
}

impl ProviderKind {
    /// Parse a provider name from a source declaration's third argument.
    /// Anything other than `core` is the default `nix`.
    pub fn parse(s: &str) -> ProviderKind {
        match s {
            "core" => ProviderKind::Core,
            "cran" => ProviderKind::Cran,
            _ => ProviderKind::Nix,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ProviderKind::Nix => "nix",
            ProviderKind::Core => "core",
            ProviderKind::Cran => "cran",
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
    /// the table for direct CLI refs (`jetpack run nixpkgs:fastfetch`).
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

/// A classified ref. `package` is the part after the first `:` — an attr name
/// for nixpkgs, an `owner/repo[/subpath]` for github, a path for `path`.
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
    /// No `:` separator at all, e.g. `fastfetch`.
    MissingSeparator(String),
    /// An empty source or package half, e.g. `:fastfetch` or `nixpkgs:`.
    EmptyHalf(String),
    /// The source prefix is neither a built-in nor a declared named source.
    /// `declared` lists the pack's named sources so the message can help.
    UnknownSource {
        source: String,
        raw: String,
        declared: Vec<String>,
    },
    /// D-MONOREF1=A: bare name with no source prefix, and no workspace member
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
    /// The registered diagnostic code for the errors that carry one. The older
    /// classifier errors render without a code (CLI-only, pre-registry); the
    /// workspace-index errors (Slice B) are registered in docs/spec/diagnostics.md.
    pub fn code(&self) -> Option<&'static str> {
        match self {
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

/// Classify a `<source>:<package/path>` ref against only the built-in sources.
/// This is the strict path for direct CLI refs.
pub fn classify(raw: &str) -> Result<RefSpec, RefError> {
    classify_in(raw, &SourceTable::empty())
}

/// Classify a ref, accepting built-in sources plus any named source declared in
/// `table` (D-JPK17). The split is on the *first* `:` so a package path may
/// itself contain a colon.
///
/// D-MONOREF1=A: also accepts the dot form `source.package` (e.g. `mono.ranker`)
/// when the left side of the first `.` matches a declared named source. The
/// colon form (`source:package`) is always tried first; the dot form is a
/// fallback when no `:` is present.
pub fn classify_in(raw: &str, table: &SourceTable) -> Result<RefSpec, RefError> {
    let raw = raw.trim();

    // Primary form: `source:package` (colon separator).
    if let Some((source, package)) = raw.split_once(Syntax::REF_SEPARATOR) {
        if source.is_empty() || package.is_empty() {
            return Err(RefError::EmptyHalf(raw.to_string()));
        }
        let src = match Source::builtin(source) {
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

    // D-MONOREF1=A: dot form `source.package` — only when the left side of the
    // first `.` is a declared named source. Built-ins (`nixpkgs`, `github`,
    // `path`) never use the dot form (they use colon).
    if let Some((source_candidate, package)) = raw.split_once('.') {
        if !source_candidate.is_empty()
            && !package.is_empty()
            && table.upstream(source_candidate).is_some()
        {
            return Ok(RefSpec {
                source: Source::Named(source_candidate.to_string()),
                package: package.to_string(),
                raw: raw.to_string(),
            });
        }
    }

    Err(RefError::MissingSeparator(raw.to_string()))
}

/// Classify a ref with workspace-member awareness (Slice B, D-MONOREF1=A).
///
/// Resolution order, first match wins:
///   1. colon form  `source:package`  — via `classify_in` (unchanged)
///   2. dot form    `source.package`  — via `classify_in` (unchanged)
///   3. path form   `infra/logging`   — exact relative-path match in the index
///   4. bare form   `logging`         — exact member-name match in the index
///
/// The colon/dot forms are tried first so an explicit source prefix always wins
/// over an accidental index collision. Only a ref with no source prefix (what
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
        // No source prefix. Consult the workspace index only when a workspace
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
// `provider@target` source refs (U6, was D-JPK18).
//
// The typed authoring surface (env.jet/pkg.jet `sources:`/`packages:`) writes source
// refs as `provider@target` — `github@owner/repo/rev`, `path@../local`,
// `nixpkgs@channel`. This is distinct from the Phase-1 command-line
// `source:package` form above (D-JPK18 keeps the colon classifier for
// compatibility): a provider ref names *where a source comes from*, while a
// `source:package` ref names *which package within a source*.
//
// This is the foundational classifier (JPK-0 / Chunk 1). Pack-file parsing and
// the user-facing diagnostics that render these errors land with the manifest
// reshape + module surface chunks; until then the typed `RefError` is internal.
// ──────────────────────────────────────────────

/// A classified `provider@target` source ref. `provider` is a built-in source
/// (github / path / nixpkgs); `target` is the upstream locator the provider
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

/// Classify a `provider@target` source ref against the built-in providers.
/// The split is on the *first* `@` so a target may itself contain `@`.
pub fn classify_provider_ref(raw: &str) -> Result<ProviderRef, RefError> {
    let raw = raw.trim();
    let (provider, target) = match raw.split_once(Syntax::REF_PROVIDER_AT) {
        Some(parts) => parts,
        None => return Err(RefError::MissingSeparator(raw.to_string())),
    };
    if provider.is_empty() || target.is_empty() {
        return Err(RefError::EmptyHalf(raw.to_string()));
    }
    let provider = match Source::builtin(provider) {
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
        let r = classify("nixpkgs:fastfetch").unwrap();
        assert_eq!(r.source, Source::Nixpkgs);
        assert_eq!(r.package, "fastfetch");
        assert_eq!(r.short_name(), "fastfetch");
    }

    #[test]
    fn classifies_github_repo() {
        let r = classify("github:halcyonomega/my-fastfetch-jet-config").unwrap();
        assert_eq!(r.source, Source::Github);
        assert_eq!(r.package, "halcyonomega/my-fastfetch-jet-config");
        assert_eq!(r.short_name(), "my-fastfetch-jet-config");
    }

    #[test]
    fn classifies_local_path() {
        let r = classify("path:./my-env").unwrap();
        assert_eq!(r.source, Source::Path);
        assert_eq!(r.package, "./my-env");
    }

    #[test]
    fn classifies_direct_cran_root_with_exact_version() {
        let r = classify("cran:jsonlite#version=1.9.0").unwrap();
        assert_eq!(r.source, Source::Cran);
        assert_eq!(r.package, "jsonlite#version=1.9.0");
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
            classify(":fastfetch"),
            Err(RefError::EmptyHalf(_))
        ));
        assert!(matches!(classify("nixpkgs:"), Err(RefError::EmptyHalf(_))));
    }

    #[test]
    fn rejects_hash_selector() {
        // Users must not type Nix's `#`; `nixpkgs#fastfetch` has no `:`.
        assert!(matches!(
            classify("nixpkgs#fastfetch"),
            Err(RefError::MissingSeparator(_))
        ));
    }

    #[test]
    fn rejects_unknown_source() {
        match classify("brew:wget") {
            Err(RefError::UnknownSource { source, .. }) => assert_eq!(source, "brew"),
            other => panic!("expected UnknownSource, got {other:?}"),
        }
    }

    #[test]
    fn provider_ref_marks_channel_selectors() {
        assert_eq!(
            classify_provider_ref("github@openai/codex#latest")
                .unwrap()
                .channel,
            Some(ChannelRef::Latest)
        );
        assert_eq!(
            classify_provider_ref("github@openai/codex#main")
                .unwrap()
                .channel,
            Some(ChannelRef::Main)
        );
        assert_eq!(
            classify_provider_ref("github@openai/codex#v0.x")
                .unwrap()
                .channel,
            Some(ChannelRef::SemverMask("v0.x".to_string()))
        );
        assert_eq!(
            classify_provider_ref("github@openai/codex#v0.50.1")
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
        let r = classify_in("stable:ripgrep", &table).unwrap();
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
        match classify_in("beta:neovim", &table) {
            Err(RefError::UnknownSource { declared, .. }) => {
                assert_eq!(declared, vec!["stable", "unstable"]);
            }
            other => panic!("expected UnknownSource, got {other:?}"),
        }
    }

    #[test]
    fn builtins_resolve_without_declaration() {
        let table = SourceTable::empty();
        assert!(classify_in("nixpkgs:fd", &table).is_ok());
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
    fn colon_form_wins_over_index() {
        // An explicit `source:package` is never shadowed by the index.
        let table =
            SourceTable::from_decls([("nixpkgs".to_string(), "u".to_string(), ProviderKind::Nix)]);
        let r = classify_with_workspace("nixpkgs:logging", &table, &ws_index()).unwrap();
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

    // ── provider@target source refs (U6) ──

    #[test]
    fn provider_ref_github_with_rev() {
        let r = classify_provider_ref("github@NixOS/nixpkgs/nixos-24.05").unwrap();
        assert_eq!(r.provider, Source::Github);
        assert_eq!(r.target, "NixOS/nixpkgs/nixos-24.05");
    }

    #[test]
    fn provider_ref_local_path() {
        let r = classify_provider_ref("path@../helpers").unwrap();
        assert_eq!(r.provider, Source::Path);
        assert_eq!(r.target, "../helpers");
    }

    #[test]
    fn provider_ref_nixpkgs_channel() {
        let r = classify_provider_ref("nixpkgs@nixpkgs-unstable").unwrap();
        assert_eq!(r.provider, Source::Nixpkgs);
        assert_eq!(r.target, "nixpkgs-unstable");
    }

    #[test]
    fn provider_ref_splits_on_first_at() {
        // A target may contain `@` (e.g. a future user@host form); only the
        // first `@` separates provider from target.
        let r = classify_provider_ref("path@a@b").unwrap();
        assert_eq!(r.provider, Source::Path);
        assert_eq!(r.target, "a@b");
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
            classify_provider_ref("@target"),
            Err(RefError::EmptyHalf(_))
        ));
        assert!(matches!(
            classify_provider_ref("github@"),
            Err(RefError::EmptyHalf(_))
        ));
    }

    #[test]
    fn provider_ref_rejects_unknown_provider() {
        match classify_provider_ref("gitlab@owner/repo") {
            Err(RefError::UnknownSource { source, .. }) => assert_eq!(source, "gitlab"),
            other => panic!("expected UnknownSource, got {other:?}"),
        }
    }
}
