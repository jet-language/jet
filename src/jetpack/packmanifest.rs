//! The Jet-syntax `pack.jet` package manifest (U1 — Cargo.toml analog).
//!
//! Unified-ecosystem §2.1: the package tier. One language everywhere — the
//! manifest is written in Jet syntax, not TOML. It holds package identity, Jet
//! library dependencies, and the optional list of public modules the package
//! exports:
//!
//! ```jet
//! package: {
//!     name:    "wordstats",
//!     version: "0.1.0",
//!     edition: "2026",
//!     license: "MIT OR Apache-2.0",
//! }
//! deps: {
//!     textkit: "1.2.0",
//!     helpers: path@../helpers,
//! }
//! exports: [module web, module cli]   // optional
//! ```
//!
//! This module is the structural parser for that shape (U1). It is std-only (I6)
//! and isolated: it does not yet replace the compiler's `jet.toml` path
//! (`manifest.rs`) — full wiring is blocked on **D-JPK23** (open decision,
//! docs/spec/syntax-decisions.md): the `jet.toml` git-dependency shape
//! (`{ git = "...", tag = "..." }` / `branch` / `rev`) has no equivalent yet
//! in the `provider@target` grammar, which only covers github.com refs with
//! one ambiguous trailing segment. Dependency values this parser *does*
//! support map cleanly: a registry version string (`"1.2.0"`) or a
//! `provider@target` source ref (`path@../local`, `github@owner/repo/rev`),
//! classified through `refspec::classify_provider_ref` (U6). The lockfile
//! side of Step 3 (unifying `jet.lock`/`pack.lock` into `.jet/lock`, U2) is
//! done — see `lock.rs`/`fetch.rs`/`loader.rs`, all reading/writing
//! `syntax::UNIFIED_LOCK_FILE`. User-facing diagnostics (I4) for this parser
//! land when D-JPK23 is ratified and the parser is wired into the loader.

use super::refspec::{self, RefError, Source};
use crate::syntax;

/// Package identity (the `package: { … }` block).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PackageMeta {
    pub name: String,
    pub version: String,
    pub edition: Option<String>,
    pub license: Option<String>,
    pub description: Option<String>,
    pub repository: Option<String>,
}

/// Where a dependency resolves from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepSource {
    /// A registry version string, e.g. `"1.2.0"`.
    Version(String),
    /// A `provider@target` source ref, e.g. `path@../helpers`.
    Provider { provider: Source, target: String },
}

/// One `name: value` entry in the `deps: { … }` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dep {
    pub name: String,
    pub source: DepSource,
}

/// A parsed `pack.jet` package manifest.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PackManifest {
    pub package: PackageMeta,
    /// Dependencies, in declaration order.
    pub deps: Vec<Dep>,
    /// Public modules this package exports (the `exports: [module x, …]` list).
    pub exports: Vec<String>,
}

/// Why a `pack.jet` package manifest could not be parsed. These are internal
/// (typed) errors for now; they become I4 diagnostics when the parser is wired
/// into the loader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    /// No `package: { … }` block at all.
    MissingPackage,
    /// `package` is missing a required `name` or `version`.
    MissingField(&'static str),
    /// A `deps` value is neither a quoted version nor a `provider@target` ref.
    BadDepValue { name: String, value: String },
    /// A `provider@target` dep ref failed to classify (U6).
    BadDepRef { name: String, err: RefError },
    /// An `exports` item is not `module <name>`.
    BadExport(String),
}

impl PackManifest {
    /// The path to the package manifest in a project dir.
    pub fn path_in(dir: &std::path::Path) -> std::path::PathBuf {
        dir.join(syntax::PACK_FILE)
    }

    /// Load and parse the package manifest in `dir`, if present.
    pub fn load(dir: &std::path::Path) -> Option<Result<PackManifest, ManifestError>> {
        let text = std::fs::read_to_string(Self::path_in(dir)).ok()?;
        Some(parse(&text))
    }
}

/// Parse a `pack.jet` package manifest from its text (U1).
pub fn parse(text: &str) -> Result<PackManifest, ManifestError> {
    let text = strip_line_comments(text);

    let package = match block_body(&text, "package", '{', '}') {
        Some(body) => parse_package(&body)?,
        None => return Err(ManifestError::MissingPackage),
    };

    let deps = match block_body(&text, "deps", '{', '}') {
        Some(body) => parse_deps(&body)?,
        None => Vec::new(),
    };

    let exports = match block_body(&text, "exports", '[', ']') {
        Some(body) => parse_exports(&body)?,
        None => Vec::new(),
    };

    Ok(PackManifest {
        package,
        deps,
        exports,
    })
}

fn parse_package(body: &str) -> Result<PackageMeta, ManifestError> {
    let mut meta = PackageMeta::default();
    let mut have_name = false;
    let mut have_version = false;
    for (key, value) in key_value_entries(body) {
        let v = unquote(&value);
        match key.as_str() {
            "name" => {
                meta.name = v;
                have_name = true;
            }
            "version" => {
                meta.version = v;
                have_version = true;
            }
            "edition" => meta.edition = Some(v),
            "license" => meta.license = Some(v),
            "description" => meta.description = Some(v),
            "repository" => meta.repository = Some(v),
            // Unknown keys are tolerated for forward-compat; the wired loader
            // will turn unknown keys into an E-coded diagnostic.
            _ => {}
        }
    }
    if !have_name {
        return Err(ManifestError::MissingField("name"));
    }
    if !have_version {
        return Err(ManifestError::MissingField("version"));
    }
    Ok(meta)
}

fn parse_deps(body: &str) -> Result<Vec<Dep>, ManifestError> {
    let mut deps = Vec::new();
    for (name, value) in key_value_entries(body) {
        let trimmed = value.trim();
        let source = if trimmed.starts_with('"') {
            DepSource::Version(unquote(trimmed))
        } else if trimmed.contains(syntax::REF_PROVIDER_AT) {
            match refspec::classify_provider_ref(trimmed) {
                Ok(r) => DepSource::Provider {
                    provider: r.provider,
                    target: r.target,
                },
                Err(err) => return Err(ManifestError::BadDepRef { name, err }),
            }
        } else {
            return Err(ManifestError::BadDepValue {
                name,
                value: trimmed.to_string(),
            });
        };
        deps.push(Dep { name, source });
    }
    Ok(deps)
}

fn parse_exports(body: &str) -> Result<Vec<String>, ManifestError> {
    let mut exports = Vec::new();
    for item in body.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        // Each item is `module <name>` (U3 module keyword) — require real
        // whitespace after `module` so `moduleweb` is not accepted.
        match item.strip_prefix(syntax::KW_MODULE) {
            Some(rest) if rest.starts_with(char::is_whitespace) && is_ident(rest.trim()) => {
                exports.push(rest.trim().to_string());
            }
            _ => return Err(ManifestError::BadExport(item.to_string())),
        }
    }
    Ok(exports)
}

// ── small structural helpers (std-only, comment-stripped input) ──────────────

/// Remove `//` line comments, preserving the rest of each line. (Block comments
/// and string-embedded `//` are out of scope for the manifest surface.)
fn strip_line_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let keep = match line.find(syntax::COMMENT_PREFIX) {
            Some(i) => &line[..i],
            None => line,
        };
        out.push_str(keep);
        out.push('\n');
    }
    out
}

/// The body inside the `open`/`close` delimiters following `key:` at the top
/// level, with balanced nesting. Returns `None` if `key:` (followed by `open`)
/// is absent.
fn block_body(text: &str, key: &str, open: char, close: char) -> Option<String> {
    let mut search_from = 0;
    while let Some(rel) = text[search_from..].find(key) {
        let at = search_from + rel;
        // Require a word boundary before `key` so `deps` doesn't match inside a
        // longer identifier.
        let preceded_ok = at == 0
            || !text[..at]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
        let after = &text[at + key.len()..];
        let after_trim = after.trim_start();
        if preceded_ok && after_trim.starts_with(':') {
            let rest = after_trim[1..].trim_start();
            if let Some(stripped) = rest.strip_prefix(open) {
                return Some(balanced(stripped, open, close));
            }
        }
        search_from = at + key.len();
    }
    None
}

/// Capture text up to the matching `close`, honoring nested `open`/`close`.
fn balanced(s: &str, open: char, close: char) -> String {
    let mut depth = 1;
    let mut out = String::new();
    for c in s.chars() {
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                break;
            }
        }
        out.push(c);
    }
    out
}

/// Split a `{ … }` body into `key: value` entries. Splits entries on commas at
/// the top nesting level (so a value may itself contain brackets), then splits
/// each entry on its first `:`.
fn key_value_entries(body: &str) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    for entry in top_level_commas(body) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        if let Some((k, v)) = entry.split_once(':') {
            entries.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    entries
}

/// Split on commas that are not nested inside `()`/`[]`/`{}`.
fn top_level_commas(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for c in body.chars() {
        match c {
            '(' | '[' | '{' => {
                depth += 1;
                cur.push(c);
            }
            ')' | ']' | '}' => {
                depth -= 1;
                cur.push(c);
            }
            ',' if depth == 0 => {
                out.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// Strip surrounding double quotes if present; otherwise return as-is, trimmed.
fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = r#"
package: {
    name:    "wordstats",
    version: "0.1.0",
    edition: "2026",
    license: "MIT OR Apache-2.0",
}
deps: {
    textkit: "1.2.0",
    helpers: path@../helpers,
}
exports: [module web, module cli]   // optional: public modules
"#;

    #[test]
    fn parses_package_block() {
        let m = parse(FULL).unwrap();
        assert_eq!(m.package.name, "wordstats");
        assert_eq!(m.package.version, "0.1.0");
        assert_eq!(m.package.edition.as_deref(), Some("2026"));
        assert_eq!(m.package.license.as_deref(), Some("MIT OR Apache-2.0"));
    }

    #[test]
    fn parses_deps_version_and_provider_ref() {
        let m = parse(FULL).unwrap();
        assert_eq!(m.deps.len(), 2);
        assert_eq!(m.deps[0].name, "textkit");
        assert_eq!(m.deps[0].source, DepSource::Version("1.2.0".into()));
        assert_eq!(m.deps[1].name, "helpers");
        assert_eq!(
            m.deps[1].source,
            DepSource::Provider {
                provider: Source::Path,
                target: "../helpers".into(),
            }
        );
    }

    #[test]
    fn parses_exports_modules() {
        let m = parse(FULL).unwrap();
        assert_eq!(m.exports, vec!["web", "cli"]);
    }

    #[test]
    fn deps_and_exports_are_optional() {
        let m = parse("package: { name: \"x\", version: \"0.0.1\" }").unwrap();
        assert!(m.deps.is_empty());
        assert!(m.exports.is_empty());
        assert_eq!(m.package.name, "x");
    }

    #[test]
    fn github_provider_dep() {
        let src = r#"
package: { name: "p", version: "0.1.0" }
deps: { up: github@NixOS/nixpkgs/nixos-24.05 }
"#;
        let m = parse(src).unwrap();
        assert_eq!(
            m.deps[0].source,
            DepSource::Provider {
                provider: Source::Github,
                target: "NixOS/nixpkgs/nixos-24.05".into(),
            }
        );
    }

    #[test]
    fn missing_package_block_errors() {
        assert_eq!(parse("deps: {}"), Err(ManifestError::MissingPackage));
    }

    #[test]
    fn missing_required_field_errors() {
        assert_eq!(
            parse("package: { name: \"x\" }"),
            Err(ManifestError::MissingField("version"))
        );
        assert_eq!(
            parse("package: { version: \"0.1.0\" }"),
            Err(ManifestError::MissingField("name"))
        );
    }

    #[test]
    fn bad_dep_value_errors() {
        // A bare token with no `@` and no quotes is not a valid dep value.
        let err = parse("package: { name: \"x\", version: \"1\" }\ndeps: { y: notaref }")
            .unwrap_err();
        assert!(matches!(err, ManifestError::BadDepValue { .. }));
    }

    #[test]
    fn bad_export_errors() {
        let err = parse("package: { name: \"x\", version: \"1\" }\nexports: [web]").unwrap_err();
        assert!(matches!(err, ManifestError::BadExport(_)));
    }

    #[test]
    fn comments_are_ignored() {
        let src = r#"
// a leading comment
package: {
    name: "x",      // inline comment
    version: "0.1.0",
}
"#;
        let m = parse(src).unwrap();
        assert_eq!(m.package.name, "x");
        assert_eq!(m.package.version, "0.1.0");
    }

    #[test]
    fn deps_prefix_does_not_match_inside_word() {
        // `dependencies:` must not be picked up as the `deps:` block.
        let src = r#"
package: { name: "x", version: "1" }
dependencies: { should_be_ignored: "9.9.9" }
"#;
        let m = parse(src).unwrap();
        assert!(m.deps.is_empty(), "deps: {:?}", m.deps);
    }
}
