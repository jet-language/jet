//! The Jet env file `env.jet` (D-JPK20/U3) — directive surface (D-JPK3).
//!
//! Phase 1 ships the directive author surface the language supports today:
//!
//! ```jet
//! // env.jet — a Jetpack project environment (dev-shell descriptor)
//! use jetpack as pkg;
//!
//! pub fn shell() -> [JSON] {
//!     return [
//!         pkg.source("nixpkgs");
//!         pkg.packages(["ripgrep", "fd", "claude-code"]);
//!         pkg.prompt("jetpack");
//!     ];
//! }
//! ```
//!
//! Phase 1 does not yet *run* the env file through the compiler; it reads the
//! directive calls structurally so `jetpack add/remove/build` can resolve and
//! edit the declared environment. The env file stays the declarative front
//! door — `add`/`remove` edit this file, never a hidden install DB (D-JPK4).

use super::RefSpec::{ProviderKind, RefSpec, Source, SourceTable};
use crate::Syntax;
use std::path::{Path, PathBuf};

/// A named source declaration: `pkg.source("<name>", "<upstream>" [, "<via>"])`.
/// `via` selects the provider (`nix` default, or `core` for first-party Jet
/// packages, R2). D-JPK17.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedSource {
    pub name: String,
    pub upstream: String,
    /// The provider name (`nix`/`core`); `None` means the default (`nix`).
    pub via: Option<String>,
}

/// The declared environment, parsed from the directive calls. Sources may be a
/// single default (`pkg.source("nixpkgs")`) and/or named declarations
/// (`pkg.source("stable", "github:…")`, D-JPK17). Package entries are written
/// either bare (resolved against the default source) or `name:package`.
///
/// `env.jet` is **purely** the dev-shell descriptor (U10): the package index
/// that maps a package name → its source lives in the source repo's
/// `pkg.jet` `packages:` block, read by the `core` provider — never here.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EnvFile {
    /// The default source for bare package entries (from a one-arg `pkg.source`).
    pub default_source: Option<String>,
    /// Named sources, in declaration order.
    pub named: Vec<NamedSource>,
    pub packages: Vec<String>,
    pub prompt: Option<String>,
}

impl EnvFile {
    /// The prompt label, defaulting to `jetpack`.
    pub fn prompt_label(&self) -> String {
        self.prompt
            .clone()
            .unwrap_or_else(|| Syntax::JETPACK_PROMPT_LABEL.to_string())
    }

    /// The default source label, defaulting to `nixpkgs`.
    pub fn source_label(&self) -> String {
        self.default_source
            .clone()
            .unwrap_or_else(|| Syntax::REF_SOURCE_NIXPKGS.to_string())
    }

    /// The named-source resolution table for this env (D-JPK17).
    pub fn source_table(&self) -> SourceTable {
        SourceTable::from_decls(self.named.iter().map(|s| {
            let via = s
                .via
                .as_deref()
                .map(ProviderKind::parse)
                .unwrap_or_default();
            (s.name.clone(), s.upstream.clone(), via)
        }))
    }

    /// Resolve one package entry to a full `<source>:<package>` ref: entries
    /// that already carry a source pass through; bare entries take the default.
    fn entry_ref(&self, entry: &str) -> String {
        if entry.contains(Syntax::REF_SEPARATOR) {
            entry.to_string()
        } else {
            format!("{}{}{}", self.source_label(), Syntax::REF_SEPARATOR, entry)
        }
    }

    /// Reconstruct the canonical refs this env declares.
    pub fn refs(&self) -> Vec<String> {
        self.packages.iter().map(|p| self.entry_ref(p)).collect()
    }

    /// Render the canonical `env.jet` text for this environment.
    pub fn render(&self) -> String {
        let prompt = self.prompt_label();
        let mut lines = Vec::new();
        for s in &self.named {
            match &s.via {
                Some(via) => lines.push(format!(
                    "        pkg.source(\"{}\", \"{}\", \"{via}\");",
                    s.name, s.upstream
                )),
                None => lines.push(format!(
                    "        pkg.source(\"{}\", \"{}\");",
                    s.name, s.upstream
                )),
            }
        }
        if let Some(default) = &self.default_source {
            lines.push(format!("        pkg.source(\"{default}\");"));
        }
        let pkgs = self
            .packages
            .iter()
            .map(|p| format!("\"{p}\""))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("        pkg.packages([{pkgs}]);"));
        lines.push(format!("        pkg.prompt(\"{prompt}\");"));
        format!(
            "// {file} — a Jetpack project environment\n\
             use jetpack as pkg;\n\
             \n\
             pub fn shell() -> [JSON] {{\n\
             \x20   return [\n\
             {body}\n\
             \x20   ];\n\
             }}\n",
            file = Syntax::ENV_FILE,
            body = lines.join("\n"),
        )
    }
}

/// Path to the env file in a project dir.
pub fn path_in(dir: &Path) -> PathBuf {
    dir.join(Syntax::ENV_FILE)
}

/// Load and parse the env file in `dir`, if present.
pub fn load(dir: &Path) -> Option<EnvFile> {
    let text = std::fs::read_to_string(path_in(dir)).ok()?;
    Some(parse(&text))
}

/// Parse the directive calls out of env-file text. Tolerant: anything it
/// doesn't recognize is ignored. `pkg.source` is read for every occurrence —
/// one arg sets the default source, two declare a named source (D-JPK17).
pub fn parse(text: &str) -> EnvFile {
    let mut default_source = None;
    let mut named = Vec::new();
    for call in all_call_args(text, Syntax::PACK_DIRECTIVE_SOURCE) {
        let args = quoted_strings(&call);
        match args.as_slice() {
            [name] => default_source = Some(name.clone()),
            [name, upstream] => named.push(NamedSource {
                name: name.clone(),
                upstream: upstream.clone(),
                via: None,
            }),
            [name, upstream, via, ..] => named.push(NamedSource {
                name: name.clone(),
                upstream: upstream.clone(),
                via: Some(via.clone()),
            }),
            _ => {}
        }
    }
    EnvFile {
        default_source,
        named,
        packages: list_arg(text, Syntax::PACK_DIRECTIVE_PACKAGES),
        prompt: string_arg(text, Syntax::PACK_DIRECTIVE_PROMPT),
    }
}

/// How a ref should be stored as a package entry: bare when it matches the
/// default source, otherwise `name:package` so the source survives.
fn entry_for(ef: &EnvFile, spec: &RefSpec) -> String {
    let source = spec.source.label();
    let is_default = match &spec.source {
        Source::Named(_) => false,
        builtin => builtin.label() == ef.source_label(),
    };
    if is_default {
        spec.package.clone()
    } else {
        format!("{source}{}{}", Syntax::REF_SEPARATOR, spec.package)
    }
}

/// Add a package (from a classified ref) to the env file in `dir`, creating
/// the file if absent. Returns the updated `EnvFile`. Idempotent.
pub fn add(dir: &Path, spec: &RefSpec) -> std::io::Result<EnvFile> {
    let mut ef = load(dir).unwrap_or_default();
    // A built-in ref with no default yet becomes the default source.
    if ef.default_source.is_none() && Source::is_builtin(spec.source.label()) {
        ef.default_source = Some(spec.source.label().to_string());
    }
    let entry = entry_for(&ef, spec);
    if !ef.packages.contains(&entry) {
        ef.packages.push(entry);
        ef.packages.sort();
    }
    std::fs::write(path_in(dir), ef.render())?;
    Ok(ef)
}

/// Remove a package from the env file in `dir`. Matches on the fully-resolved
/// ref, so it works whether the entry was stored bare or `name:package`.
pub fn remove(dir: &Path, spec: &RefSpec) -> std::io::Result<(EnvFile, bool)> {
    let mut ef = load(dir).unwrap_or_default();
    let target = format!(
        "{}{}{}",
        spec.source.label(),
        Syntax::REF_SEPARATOR,
        spec.package
    );
    let before = ef.packages.len();
    let resolved: Vec<String> = ef.packages.iter().map(|p| ef.entry_ref(p)).collect();
    let mut keep = Vec::new();
    for (entry, full) in ef.packages.iter().zip(resolved.iter()) {
        if full != &target {
            keep.push(entry.clone());
        }
    }
    ef.packages = keep;
    let removed = ef.packages.len() != before;
    if removed {
        std::fs::write(path_in(dir), ef.render())?;
    }
    Ok((ef, removed))
}

// ── directive extraction ─────────────────────────────────────────────────

/// Find `name("...")` and return the first quoted string argument.
fn string_arg(text: &str, name: &str) -> Option<String> {
    let inside = call_args(text, name)?;
    quoted_strings(&inside).into_iter().next()
}

/// Find `name([...])` and return all quoted strings in the list.
fn list_arg(text: &str, name: &str) -> Vec<String> {
    match call_args(text, name) {
        Some(inside) => quoted_strings(&inside),
        None => Vec::new(),
    }
}

/// The inner args of *every* `name(...)` call in `text`, in order. Used for
/// `pkg.source`, which may appear several times (D-JPK17).
fn all_call_args(text: &str, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(pos) = rest.find(name) {
        // Parse from this occurrence; if it has a paren group, record it.
        if let Some(args) = call_args(rest, name) {
            out.push(args);
        }
        // Advance past this occurrence's name to find the next.
        rest = &rest[pos + name.len()..];
    }
    out
}

/// The text between the matched `(` and its closing `)` for a `name(...)` call.
fn call_args(text: &str, name: &str) -> Option<String> {
    let start = text.find(name)? + name.len();
    let bytes: Vec<char> = text[start..].chars().collect();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_whitespace() {
        i += 1;
    }
    if bytes.get(i) != Some(&'(') {
        return None;
    }
    i += 1;
    let mut depth = 1;
    let mut out = String::new();
    while i < bytes.len() {
        match bytes[i] {
            '(' => {
                depth += 1;
                out.push('(');
            }
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(out);
                }
                out.push(')');
            }
            c => out.push(c),
        }
        i += 1;
    }
    None
}

/// Extract every `"..."` string literal from a fragment, in order.
fn quoted_strings(fragment: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = fragment.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' {
            let mut s = String::new();
            for c in chars.by_ref() {
                if c == '"' {
                    break;
                }
                s.push(c);
            }
            out.push(s);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::RefSpec::{classify, classify_in};
    use super::*;

    const SAMPLE: &str = r#"
use jetpack as pkg;
pub fn shell() -> [JSON] {
    return [
        pkg.source("nixpkgs");
        pkg.packages(["ripgrep", "fd", "claude-code"]);
        pkg.prompt("jetpack");
    ];
}
"#;

    const NAMED: &str = r#"
use jetpack as pkg;
pub fn shell() -> [JSON] {
    return [
        pkg.source("stable", "github:NixOS/nixpkgs/nixos-24.05");
        pkg.source("unstable", "github:NixOS/nixpkgs/nixpkgs-unstable");
        pkg.packages(["stable:ripgrep", "unstable:neovim"]);
        pkg.prompt("jetpack");
    ];
}
"#;

    #[test]
    fn parses_directives() {
        let ef = parse(SAMPLE);
        assert_eq!(ef.default_source.as_deref(), Some("nixpkgs"));
        assert!(ef.named.is_empty());
        assert_eq!(ef.packages, vec!["ripgrep", "fd", "claude-code"]);
        assert_eq!(ef.prompt.as_deref(), Some("jetpack"));
        assert_eq!(
            ef.refs(),
            vec!["nixpkgs:ripgrep", "nixpkgs:fd", "nixpkgs:claude-code"]
        );
    }

    #[test]
    fn parses_named_sources() {
        let ef = parse(NAMED);
        assert_eq!(ef.named.len(), 2);
        assert_eq!(ef.named[0].name, "stable");
        assert_eq!(ef.named[0].upstream, "github:NixOS/nixpkgs/nixos-24.05");
        assert_eq!(ef.named[0].via, None);
        assert_eq!(ef.named[1].name, "unstable");
        assert_eq!(ef.refs(), vec!["stable:ripgrep", "unstable:neovim"]);
        // Every declared name classifies against the env's table.
        let table = ef.source_table();
        for r in ef.refs() {
            assert!(classify_in(&r, &table).is_ok(), "ref {r} should classify");
        }
    }

    #[test]
    fn parses_core_source() {
        // U10: `env.jet` is purely the dev-shell descriptor. A `core` named
        // source still declares its provider via the `via` marker; the
        // name→source package index lives in the repo's `pkg.jet`, not here.
        let repo = r#"
use jetpack as pkg;
pub fn shell() -> [JSON] {
    return [
        pkg.source("mine", "path:./jet-pkgs", "core");
        pkg.packages(["mine:hello"]);
    ];
}
"#;
        let ef = parse(repo);
        assert_eq!(ef.named.len(), 1);
        assert_eq!(ef.named[0].via.as_deref(), Some("core"));
        // The `core` provider is selected for the named source.
        assert_eq!(
            ef.source_table().provider("mine"),
            super::super::RefSpec::ProviderKind::Core
        );
        assert_eq!(ef.packages, vec!["mine:hello"]);
    }

    #[test]
    fn render_roundtrips_named() {
        let ef = parse(NAMED);
        let rendered = ef.render();
        assert!(rendered.contains("pub fn shell() -> [JSON]"));
        for line in rendered
            .lines()
            .filter(|line| line.trim_start().starts_with("pkg."))
        {
            assert!(line.trim_end().ends_with(';'), "directive line: {line}");
            assert!(!line.trim_end().ends_with(','), "directive line: {line}");
        }
        let ef2 = parse(&rendered);
        assert_eq!(ef.named, ef2.named);
        assert_eq!(ef.default_source, ef2.default_source);
        assert_eq!(ef.refs(), ef2.refs());
    }

    #[test]
    fn render_roundtrips() {
        let ef = parse(SAMPLE);
        let ef2 = parse(&ef.render());
        assert_eq!(ef.default_source, ef2.default_source);
        assert_eq!(ef.prompt, ef2.prompt);
        let mut a = ef.packages.clone();
        let mut b = ef2.packages.clone();
        a.sort();
        b.sort();
        assert_eq!(a, b);
    }

    #[test]
    fn defaults_when_empty() {
        let ef = EnvFile::default();
        assert_eq!(ef.prompt_label(), "jetpack");
        assert_eq!(ef.source_label(), "nixpkgs");
    }

    #[test]
    fn add_creates_and_dedupes() {
        let dir = scratch();
        let ef = add(&dir, &classify("nixpkgs:ripgrep").unwrap()).unwrap();
        assert_eq!(ef.packages, vec!["ripgrep"]);
        // adding again is a no-op
        let ef = add(&dir, &classify("nixpkgs:ripgrep").unwrap()).unwrap();
        assert_eq!(ef.packages, vec!["ripgrep"]);
        let ef = add(&dir, &classify("nixpkgs:fd").unwrap()).unwrap();
        assert_eq!(ef.packages, vec!["fd", "ripgrep"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_edits_file() {
        let dir = scratch();
        add(&dir, &classify("nixpkgs:ripgrep").unwrap()).unwrap();
        add(&dir, &classify("nixpkgs:fd").unwrap()).unwrap();
        let (ef, removed) = remove(&dir, &classify("nixpkgs:fd").unwrap()).unwrap();
        assert!(removed);
        assert_eq!(ef.packages, vec!["ripgrep"]);
        let (_ef, removed) = remove(&dir, &classify("nixpkgs:nope").unwrap()).unwrap();
        assert!(!removed);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn add_named_source_entry_keeps_prefix() {
        let dir = scratch();
        std::fs::write(path_in(&dir), parse(NAMED).render()).unwrap();
        let table = parse(NAMED).source_table();
        // Adding under a declared named source stores it prefixed and preserves
        // the source declarations.
        let ef = add(&dir, &classify_in("unstable:fd", &table).unwrap()).unwrap();
        assert!(ef.packages.contains(&"unstable:fd".to_string()));
        assert_eq!(ef.named.len(), 2, "named sources must survive an edit");
        // Removing it by its full ref works.
        let (ef, removed) = remove(&dir, &classify_in("unstable:fd", &table).unwrap()).unwrap();
        assert!(removed);
        assert!(!ef.packages.contains(&"unstable:fd".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn scratch() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p =
            std::env::temp_dir().join(format!("jpk-env-{nanos}-{:?}", std::thread::current().id()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}
