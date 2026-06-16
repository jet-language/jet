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
//!     textkit:  "1.2.0",
//!     helpers:  path@../helpers,
//!     parsekit: { git: "https://github.com/acme/parsekit", tag: "v0.4.1" },
//! }
//! exports: [module web, module cli]   // optional
//! ```
//!
//! This module is the structural parser for that shape (U1). It is std-only
//! (I6). Dependency values are a registry version string (`"1.2.0"`), a
//! `provider@target` source ref (`path@../local`, `github@owner/repo/rev`,
//! classified through `refspec::classify_provider_ref`, U6), or an inline
//! git struct (`{ git: "<url>", tag/branch/rev: "<value>" }`, D-JPK23 —
//! generalizes to any git remote, not just GitHub). `to_manifest` converts a
//! parsed `PackManifest` into the compiler's `manifest::Manifest`, the type
//! `loader.rs`/`fetch.rs`/`lock.rs` operate on.

use super::refspec::{self, RefError, Source};
use crate::diag::Diagnostic;
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
    /// Toolchain constraint, e.g. `jet: ">=1.0.0"` (E1208).
    pub jet_constraint: Option<String>,
}

/// Where a dependency resolves from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepSource {
    /// A registry version string, e.g. `"1.2.0"`.
    Version(String),
    /// A `provider@target` source ref, e.g. `path@../helpers`.
    Provider { provider: Source, target: String },
    /// An inline git dependency (D-JPK23): any remote, with an explicit
    /// selector — `{ git: "<url>", tag/branch/rev: "<value>" }`.
    Git {
        url: String,
        selector: crate::manifest::GitSelector,
    },
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
    /// An inline git dep (D-JPK23) is missing `git`, or doesn't have exactly
    /// one of `tag`/`branch`/`rev`.
    BadGitDep { name: String, reason: &'static str },
    /// An `exports` item is not `module <name>`.
    BadExport(String),
    /// A reserved top-level key (`dev_deps`/`patch`/`workspace`) was used
    /// non-empty (carries forward jet.toml's reserved-section guard, S52/E1209).
    ReservedSection(&'static str),
}

/// Top-level keys reserved for a future Jet feature; using them non-empty
/// today is an error (carries forward jet.toml's `dev-dependencies`/`patch`/
/// `workspace` guard, S52/E1209).
const RESERVED_SECTIONS: &[&str] = &["dev_deps", "patch", "workspace"];

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

    for &section in RESERVED_SECTIONS {
        if let Some(body) = block_body(&text, section, '{', '}') {
            if !body.trim().is_empty() {
                return Err(ManifestError::ReservedSection(section));
            }
        }
    }

    Ok(PackManifest {
        package,
        deps,
        exports,
    })
}

/// Convert a parsed `PackManifest` into the compiler's `manifest::Manifest`
/// — the type `loader.rs`/`fetch.rs`/`lock.rs` operate on. `raw` is the
/// original `pack.jet` text (kept for comment-preserving `jet add`/`remove`
/// edits, mirroring the old `jet.toml` `Manifest::raw`).
pub fn to_manifest(pm: &PackManifest, raw: &str) -> Result<crate::manifest::Manifest, Diagnostic> {
    use crate::manifest::{DepSpec, GitSelector, Manifest, PackageMeta as MPackageMeta};
    use std::collections::BTreeMap;

    let mut dependencies = BTreeMap::new();
    for dep in &pm.deps {
        let spec = match &dep.source {
            DepSource::Version(v) => DepSpec::Registry(v.clone()),
            DepSource::Git { url, selector } => DepSpec::Git {
                url: url.clone(),
                selector: match selector {
                    GitSelector::Tag(t) => GitSelector::Tag(t.clone()),
                    GitSelector::Branch(b) => GitSelector::Branch(b.clone()),
                    GitSelector::Rev(r) => GitSelector::Rev(r.clone()),
                },
            },
            DepSource::Provider {
                provider: Source::Path,
                target,
            } => DepSpec::Path { path: target.clone() },
            DepSource::Provider {
                provider: Source::Github,
                target,
            } => {
                let Some((owner_repo, rev)) = target.rsplit_once('/') else {
                    return Err(bad_dep_shape(
                        &dep.name,
                        "a `github@owner/repo/rev` dependency needs a pinned rev as its last segment; use the inline `{ git: \"...\", branch/tag: \"...\" }` form to track a moving branch or tag",
                    ));
                };
                DepSpec::Git {
                    url: format!("https://github.com/{owner_repo}"),
                    selector: GitSelector::Rev(rev.to_string()),
                }
            }
            DepSource::Provider { provider, .. } => {
                return Err(bad_dep_shape(
                    &dep.name,
                    &format!(
                        "`{}` is not a valid source for a Jet library dependency — use `path@`, `github@`, or an inline git struct",
                        provider.label()
                    ),
                ));
            }
        };
        dependencies.insert(dep.name.clone(), spec);
    }

    Ok(Manifest {
        package: MPackageMeta {
            name: pm.package.name.clone(),
            version: pm.package.version.clone(),
            jet_constraint: pm.package.jet_constraint.clone(),
            description: pm.package.description.clone(),
            license: pm.package.license.clone(),
            repository: pm.package.repository.clone(),
        },
        dependencies,
        dependencies_rust: BTreeMap::new(),
        raw: raw.to_string(),
    })
}

fn bad_dep_shape(name: &str, why: &str) -> Diagnostic {
    Diagnostic::error(
        "E1206",
        format!("dependency `{name}` has an invalid shape"),
        why.to_string(),
        "see docs/spec/syntax-decisions.md D-JPK23 for the dependency ref forms".to_string(),
        None,
    )
}

/// Generate a `pack.jet` template for `jet new`.
pub fn new_template(name: &str, annotated: bool) -> String {
    let ver = crate::manifest::COMPILER_VERSION;
    if annotated {
        format!(
            r#"package: {{
    name:    "{name}",
    version: "0.1.0",
    jet:     ">={ver}",
    description: "",
    license: "MIT OR Apache-2.0",
    repository: "",
}}

// Jet package dependencies:
// deps: {{
//     helpers:  path@../helpers,
//     parsekit: {{ git: "https://github.com/acme/parsekit", tag: "v0.4.1" }},
// }}
"#
        )
    } else {
        format!(
            r#"package: {{
    name:    "{name}",
    version: "0.1.0",
    jet:     ">={ver}",
    description: "",
    license: "MIT OR Apache-2.0",
    repository: "",
}}

deps: {{
}}
"#
        )
    }
}

/// Render a compiler-side `DepSpec` back into `pack.jet` dep-value syntax.
fn render_dep_spec(spec: &crate::manifest::DepSpec) -> String {
    use crate::manifest::{DepSpec, GitSelector};
    match spec {
        DepSpec::Registry(v) => format!("\"{v}\""),
        DepSpec::Path { path } => format!("path@{path}"),
        DepSpec::Git { url, selector } => {
            let sel = match selector {
                GitSelector::Tag(t) => format!("tag: \"{t}\""),
                GitSelector::Branch(b) => format!("branch: \"{b}\""),
                GitSelector::Rev(r) => format!("rev: \"{r}\""),
            };
            format!("{{ git: \"{url}\", {sel} }}")
        }
    }
}

/// Insert or update a dependency in the `deps: { … }` block, preserving
/// comments and existing entries. Creates the block if absent. Mirrors the
/// old jet.toml `add_dependency`, but for Jet-syntax `deps:` blocks.
pub fn add_dep(raw: &str, name: &str, spec: &crate::manifest::DepSpec) -> String {
    let line = format!("    {name}: {},", render_dep_spec(spec));
    insert_or_replace_in_block(raw, "deps", name, &line)
}

/// Remove a dependency from `deps: { … }`, preserving comments.
pub fn remove_dep(raw: &str, name: &str) -> String {
    remove_from_block(raw, "deps", name)
}

/// The `[start, end)` line range of `key: { … }`'s body (the lines strictly
/// between the opening and matching closing brace), tracking brace depth so
/// nested structs (e.g. an inline git dep) don't confuse the boundary.
fn block_line_range(lines: &[String], key: &str) -> Option<(usize, usize)> {
    let header = format!("{key}:");
    let mut start: Option<usize> = None;
    let mut depth = 0i32;
    for (i, line) in lines.iter().enumerate() {
        if start.is_none() {
            let trimmed = line.trim_start();
            if trimmed.starts_with(&header) && trimmed[header.len()..].trim_start().starts_with('{')
            {
                depth = brace_delta(line);
                start = Some(i + 1);
                if depth <= 0 {
                    return Some((i + 1, i + 1));
                }
            }
            continue;
        }
        depth += brace_delta(line);
        if depth <= 0 {
            return Some((start.unwrap(), i));
        }
    }
    None
}

fn brace_delta(line: &str) -> i32 {
    line.matches('{').count() as i32 - line.matches('}').count() as i32
}

/// Insert or replace a `name: …` entry inside the `key: { … }` block,
/// creating the block (appended at end of file) if it doesn't exist yet.
fn insert_or_replace_in_block(raw: &str, key: &str, name: &str, new_line: &str) -> String {
    let lines: Vec<String> = raw.lines().map(str::to_string).collect();
    let mut out = lines.clone();

    if let Some((start, end)) = block_line_range(&lines, key) {
        let mut existing: Option<usize> = None;
        for i in start..end {
            let trimmed = lines[i].trim_start();
            if let Some((k, _)) = trimmed.split_once(':') {
                if k.trim() == name {
                    existing = Some(i);
                    break;
                }
            }
        }
        if let Some(i) = existing {
            out[i] = new_line.to_string();
        } else {
            out.insert(end, new_line.to_string());
        }
    } else {
        if !raw.is_empty() && !raw.ends_with('\n') {
            out.push(String::new());
        }
        out.push(String::new());
        out.push(format!("{key}: {{"));
        out.push(new_line.to_string());
        out.push("}".to_string());
    }

    let mut result = out.join("\n");
    if raw.ends_with('\n') && !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

/// Remove a `name: …` entry from the `key: { … }` block, preserving comments
/// and every other entry.
fn remove_from_block(raw: &str, key: &str, name: &str) -> String {
    let lines: Vec<String> = raw.lines().map(str::to_string).collect();
    let Some((start, end)) = block_line_range(&lines, key) else {
        return raw.to_string();
    };
    let mut out = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        if i >= start && i < end {
            let trimmed = line.trim_start();
            if let Some((k, _)) = trimmed.split_once(':') {
                if k.trim() == name {
                    continue;
                }
            }
        }
        out.push(line.clone());
    }
    let mut result = out.join("\n");
    if raw.ends_with('\n') && !result.ends_with('\n') {
        result.push('\n');
    }
    result
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
            "jet" => meta.jet_constraint = Some(v),
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
        } else if let Some(inner) = trimmed.strip_prefix('{') {
            let inner = inner.strip_suffix('}').unwrap_or(inner);
            parse_git_dep(&name, inner)?
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

/// Parse an inline git dependency's body (the text inside `{ … }`):
/// `git: "<url>", tag/branch/rev: "<value>"` — exactly one selector (D-JPK23).
fn parse_git_dep(name: &str, body: &str) -> Result<DepSource, ManifestError> {
    let mut url = None;
    let mut tag = None;
    let mut branch = None;
    let mut rev = None;
    for (key, value) in key_value_entries(body) {
        let v = unquote(&value);
        match key.as_str() {
            "git" => url = Some(v),
            "tag" => tag = Some(v),
            "branch" => branch = Some(v),
            "rev" => rev = Some(v),
            _ => {}
        }
    }
    let Some(url) = url else {
        return Err(ManifestError::BadGitDep {
            name: name.to_string(),
            reason: "missing `git` field",
        });
    };
    let selector = match (tag, branch, rev) {
        (Some(t), None, None) => crate::manifest::GitSelector::Tag(t),
        (None, Some(b), None) => crate::manifest::GitSelector::Branch(b),
        (None, None, Some(r)) => crate::manifest::GitSelector::Rev(r),
        (None, None, None) => {
            return Err(ManifestError::BadGitDep {
                name: name.to_string(),
                reason: "must have exactly one of `tag`, `branch`, `rev`",
            });
        }
        _ => {
            return Err(ManifestError::BadGitDep {
                name: name.to_string(),
                reason: "must have exactly one of `tag`, `branch`, `rev`",
            });
        }
    };
    Ok(DepSource::Git { url, selector })
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
        out.push_str(&strip_line_comment(line));
        out.push('\n');
    }
    out
}

/// Find the `//` that starts a line comment, ignoring one embedded in a
/// quoted string (e.g. a git URL: `"https://github.com/acme/parsekit"`).
fn strip_line_comment(line: &str) -> &str {
    let mut in_string = false;
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => in_string = !in_string,
            b'/' if !in_string && bytes.get(i + 1) == Some(&b'/') => return &line[..i],
            _ => {}
        }
        i += 1;
    }
    line
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

    // ── inline git deps (D-JPK23) ──

    #[test]
    fn git_dep_tag() {
        let src = r#"
package: { name: "p", version: "0.1.0" }
deps: { parsekit: { git: "https://github.com/acme/parsekit", tag: "v0.4.1" } }
"#;
        let m = parse(src).unwrap();
        assert_eq!(
            m.deps[0].source,
            DepSource::Git {
                url: "https://github.com/acme/parsekit".into(),
                selector: crate::manifest::GitSelector::Tag("v0.4.1".into()),
            }
        );
    }

    #[test]
    fn git_dep_branch() {
        let src = r#"
package: { name: "p", version: "0.1.0" }
deps: { nightly: { git: "https://github.com/acme/nightly", branch: "main" } }
"#;
        let m = parse(src).unwrap();
        assert_eq!(
            m.deps[0].source,
            DepSource::Git {
                url: "https://github.com/acme/nightly".into(),
                selector: crate::manifest::GitSelector::Branch("main".into()),
            }
        );
    }

    #[test]
    fn git_dep_rev_and_non_github_remote() {
        let src = r#"
package: { name: "p", version: "0.1.0" }
deps: { selfhost: { git: "https://git.example.com/acme/thing", rev: "abc123" } }
"#;
        let m = parse(src).unwrap();
        assert_eq!(
            m.deps[0].source,
            DepSource::Git {
                url: "https://git.example.com/acme/thing".into(),
                selector: crate::manifest::GitSelector::Rev("abc123".into()),
            }
        );
    }

    #[test]
    fn git_dep_missing_git_field_errors() {
        let src = r#"
package: { name: "p", version: "0.1.0" }
deps: { bad: { tag: "v1.0.0" } }
"#;
        let err = parse(src).unwrap_err();
        assert!(matches!(err, ManifestError::BadGitDep { name, .. } if name == "bad"));
    }

    #[test]
    fn git_dep_missing_selector_errors() {
        let src = r#"
package: { name: "p", version: "0.1.0" }
deps: { bad: { git: "https://example.com/x" } }
"#;
        let err = parse(src).unwrap_err();
        assert!(matches!(err, ManifestError::BadGitDep { name, .. } if name == "bad"));
    }

    #[test]
    fn git_dep_two_selectors_errors() {
        let src = r#"
package: { name: "p", version: "0.1.0" }
deps: { bad: { git: "https://example.com/x", tag: "v1", branch: "main" } }
"#;
        let err = parse(src).unwrap_err();
        assert!(matches!(err, ManifestError::BadGitDep { name, .. } if name == "bad"));
    }

    #[test]
    fn mixed_dep_kinds_in_one_block() {
        let src = r#"
package: { name: "p", version: "0.1.0" }
deps: {
    textkit:  "1.2.0",
    helpers:  path@../helpers,
    parsekit: { git: "https://github.com/acme/parsekit", tag: "v0.4.1" },
}
"#;
        let m = parse(src).unwrap();
        assert_eq!(m.deps.len(), 3);
        assert_eq!(m.deps[0].source, DepSource::Version("1.2.0".into()));
        assert!(matches!(m.deps[1].source, DepSource::Provider { .. }));
        assert!(matches!(m.deps[2].source, DepSource::Git { .. }));
    }

    #[test]
    fn reserved_section_nonempty_errors() {
        let src = r#"
package: { name: "x", version: "1" }
workspace: { members: "foo" }
"#;
        let err = parse(src).unwrap_err();
        assert_eq!(err, ManifestError::ReservedSection("workspace"));
    }

    #[test]
    fn reserved_section_empty_is_fine() {
        let src = r#"
package: { name: "x", version: "1" }
workspace: {}
"#;
        assert!(parse(src).is_ok());
    }

    // ── to_manifest conversion ──

    #[test]
    fn to_manifest_converts_version_and_path_deps() {
        let m = parse(FULL).unwrap();
        let mf = to_manifest(&m, "raw text").unwrap();
        assert_eq!(mf.package.name, "wordstats");
        assert_eq!(
            mf.dependencies.get("textkit"),
            Some(&crate::manifest::DepSpec::Registry("1.2.0".into()))
        );
        assert_eq!(
            mf.dependencies.get("helpers"),
            Some(&crate::manifest::DepSpec::Path {
                path: "../helpers".into()
            })
        );
    }

    #[test]
    fn to_manifest_converts_inline_git_dep() {
        let src = r#"
package: { name: "p", version: "0.1.0" }
deps: { parsekit: { git: "https://github.com/acme/parsekit", tag: "v0.4.1" } }
"#;
        let m = parse(src).unwrap();
        let mf = to_manifest(&m, src).unwrap();
        assert_eq!(
            mf.dependencies.get("parsekit"),
            Some(&crate::manifest::DepSpec::Git {
                url: "https://github.com/acme/parsekit".into(),
                selector: crate::manifest::GitSelector::Tag("v0.4.1".into()),
            })
        );
    }

    #[test]
    fn to_manifest_converts_github_provider_ref_as_pinned_rev() {
        let src = r#"
package: { name: "p", version: "0.1.0" }
deps: { up: github@NixOS/nixpkgs/nixos-24.05 }
"#;
        let m = parse(src).unwrap();
        let mf = to_manifest(&m, src).unwrap();
        assert_eq!(
            mf.dependencies.get("up"),
            Some(&crate::manifest::DepSpec::Git {
                url: "https://github.com/NixOS/nixpkgs".into(),
                selector: crate::manifest::GitSelector::Rev("nixos-24.05".into()),
            })
        );
    }

    #[test]
    fn to_manifest_rejects_nixpkgs_provider_dep() {
        let src = r#"
package: { name: "p", version: "0.1.0" }
deps: { x: nixpkgs@fastfetch }
"#;
        let m = parse(src).unwrap();
        let err = to_manifest(&m, src).unwrap_err();
        assert_eq!(err.code, "E1206");
    }

    #[test]
    fn to_manifest_carries_jet_constraint() {
        let src = r#"
package: { name: "p", version: "0.1.0", jet: ">=1.0.0" }
"#;
        let m = parse(src).unwrap();
        let mf = to_manifest(&m, src).unwrap();
        assert_eq!(mf.package.jet_constraint.as_deref(), Some(">=1.0.0"));
    }

    // ── template + comment-preserving edits ──

    #[test]
    fn template_plain_parses() {
        let raw = new_template("myapp", false);
        let m = parse(&raw).expect("plain template should parse");
        assert_eq!(m.package.name, "myapp");
        assert_eq!(m.package.version, "0.1.0");
        assert!(m.package.jet_constraint.is_some());
    }

    #[test]
    fn template_annotated_has_dep_comment_and_parses() {
        let raw = new_template("myapp", true);
        assert!(raw.contains("// Jet package dependencies:"), "{}", raw);
        let m = parse(&raw).expect("annotated template should parse");
        assert_eq!(m.package.name, "myapp");
    }

    #[test]
    fn add_dep_creates_block_when_absent() {
        let raw = new_template("myapp", true); // deps: block is commented out
        let updated = add_dep(
            &raw,
            "helpers",
            &crate::manifest::DepSpec::Path {
                path: "../helpers".into(),
            },
        );
        let m = parse(&updated).unwrap();
        assert_eq!(
            m.deps.iter().find(|d| d.name == "helpers").map(|d| &d.source),
            Some(&DepSource::Provider {
                provider: Source::Path,
                target: "../helpers".into(),
            })
        );
    }

    #[test]
    fn add_dep_inserts_into_existing_block_and_replaces() {
        let raw = "package: { name: \"x\", version: \"1\" }\n\ndeps: {\n    a: \"1.0.0\",\n}\n";
        let updated = add_dep(
            raw,
            "b",
            &crate::manifest::DepSpec::Path { path: "../b".into() },
        );
        let m = parse(&updated).unwrap();
        assert_eq!(m.deps.len(), 2);

        // Replacing an existing dep updates in place rather than duplicating.
        let updated2 = add_dep(
            &updated,
            "a",
            &crate::manifest::DepSpec::Registry("2.0.0".into()),
        );
        let m2 = parse(&updated2).unwrap();
        assert_eq!(m2.deps.len(), 2);
        assert_eq!(
            m2.deps.iter().find(|d| d.name == "a").map(|d| &d.source),
            Some(&DepSource::Version("2.0.0".into()))
        );
    }

    #[test]
    fn remove_dep_drops_only_named_entry() {
        let raw = "package: { name: \"x\", version: \"1\" }\n\ndeps: {\n    a: \"1.0.0\",\n    b: \"2.0.0\",\n}\n";
        let updated = remove_dep(raw, "a");
        let m = parse(&updated).unwrap();
        assert_eq!(m.deps.len(), 1);
        assert_eq!(m.deps[0].name, "b");
    }
}
