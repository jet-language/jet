//! Jetpack ref classifier: `<source>:<package/path>` (D-JPK7/15).
//!
//! The only public package syntax in Jetpack. Users never type Nix's `#`
//! selector — Jetpack translates `:` into the provider's form internally.
//! Examples:
//!   `nixpkgs:fastfetch`
//!   `github:halcyonomega/my-fastfetch-jet-config`
//!   `path:./my-env`

use crate::syntax;
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
    /// A pack-declared named source, e.g. `stable` → a pinned nixpkgs (D-JPK17).
    Named(String),
}

impl Source {
    /// The source token as written before the `:` in a ref.
    pub fn label(&self) -> &str {
        match self {
            Source::Nixpkgs => syntax::REF_SOURCE_NIXPKGS,
            Source::Github => syntax::REF_SOURCE_GITHUB,
            Source::Path => syntax::REF_SOURCE_PATH,
            Source::Named(name) => name,
        }
    }

    /// Whether `name` is one of the built-in source keywords.
    pub fn is_builtin(name: &str) -> bool {
        name == syntax::REF_SOURCE_NIXPKGS
            || name == syntax::REF_SOURCE_GITHUB
            || name == syntax::REF_SOURCE_PATH
    }

    fn builtin(name: &str) -> Option<Source> {
        match name {
            n if n == syntax::REF_SOURCE_NIXPKGS => Some(Source::Nixpkgs),
            n if n == syntax::REF_SOURCE_GITHUB => Some(Source::Github),
            n if n == syntax::REF_SOURCE_PATH => Some(Source::Path),
            _ => None,
        }
    }
}

/// Which backend realizes a source: the `nix` compatibility provider or the
/// first-party `core` provider (R2). Default is `nix` (R1 behavior).
///
/// `Infer` is a third, *unresolved* state used by the typed surface (U9): a
/// `github@…` source's kind can't be known during pure `evaluate_env`
/// evaluation — it depends on whether the remote repo carries a `pack.jet`,
/// which only a realize-time probe (with the offline flag + source cache) can
/// answer. `provider::resolve_kind` turns `Infer` into a concrete `Nix`/`Core`
/// when realization runs; it never reaches a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProviderKind {
    #[default]
    Nix,
    Core,
    /// Decide `Nix` vs `Core` at realize time by peeking the source's
    /// `pack.jet` (U9). Only the typed `github@…` surface produces this.
    Infer,
}

impl ProviderKind {
    /// Parse a provider name from a source declaration's third argument.
    /// Anything other than `core` is the default `nix`.
    pub fn parse(s: &str) -> ProviderKind {
        match s {
            "core" => ProviderKind::Core,
            _ => ProviderKind::Nix,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ProviderKind::Nix => "nix",
            ProviderKind::Core => "core",
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
/// diagnostic (see `output::ref_error`).
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
}

/// Classify a `<source>:<package/path>` ref against only the built-in sources.
/// This is the strict path for direct CLI refs.
pub fn classify(raw: &str) -> Result<RefSpec, RefError> {
    classify_in(raw, &SourceTable::empty())
}

/// Classify a ref, accepting built-in sources plus any named source declared in
/// `table` (D-JPK17). The split is on the *first* `:` so a package path may
/// itself contain a colon.
pub fn classify_in(raw: &str, table: &SourceTable) -> Result<RefSpec, RefError> {
    let raw = raw.trim();
    let (source, package) = match raw.split_once(syntax::REF_SEPARATOR) {
        Some(parts) => parts,
        None => return Err(RefError::MissingSeparator(raw.to_string())),
    };
    if source.is_empty() || package.is_empty() {
        return Err(RefError::EmptyHalf(raw.to_string()));
    }
    let source = match Source::builtin(source) {
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
    Ok(RefSpec {
        source,
        package: package.to_string(),
        raw: raw.to_string(),
    })
}

// ──────────────────────────────────────────────
// `provider@target` source refs (U6, was D-JPK18).
//
// The typed authoring surface (env.jet/pack.jet `sources:`/`packages:`) writes source
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
    pub raw: String,
}

/// Classify a `provider@target` source ref against the built-in providers.
/// The split is on the *first* `@` so a target may itself contain `@`.
pub fn classify_provider_ref(raw: &str) -> Result<ProviderRef, RefError> {
    let raw = raw.trim();
    let (provider, target) = match raw.split_once(syntax::REF_PROVIDER_AT) {
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
    Ok(ProviderRef {
        provider,
        target: target.to_string(),
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
