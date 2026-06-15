//! The Jet pack file `pack.jet` (D-JPK8/13) — directive surface (D-JPK3).
//!
//! Phase 1 ships the directive author surface the language supports today:
//!
//! ```jet
//! // pack.jet — a Jetpack dev environment (Jet's flake equivalent)
//! import jetpack as pkg;
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
//! Phase 1 does not yet *run* the pack file through the compiler; it reads the
//! directive calls structurally so `jetpack add/remove/build` can resolve and
//! edit the declared environment. The pack file stays the declarative front
//! door — `add`/`remove` edit this file, never a hidden install DB (D-JPK4).

use super::refspec::{ProviderKind, RefSpec, Source, SourceTable};
use crate::syntax;
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
/// A first-party package *repo*'s pack file also declares the packages it
/// provides via `pkg.package(...)` (R2), read into `provides`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PackFile {
    /// The default source for bare package entries (from a one-arg `pkg.source`).
    pub default_source: Option<String>,
    /// Named sources, in declaration order.
    pub named: Vec<NamedSource>,
    pub packages: Vec<String>,
    pub prompt: Option<String>,
    /// First-party packages this repo provides: `(name, source-subpath)` (R2).
    pub provides: Vec<(String, String)>,
}

impl PackFile {
    /// The prompt label, defaulting to `jetpack`.
    pub fn prompt_label(&self) -> String {
        self.prompt
            .clone()
            .unwrap_or_else(|| syntax::JETPACK_PROMPT_LABEL.to_string())
    }

    /// The default source label, defaulting to `nixpkgs`.
    pub fn source_label(&self) -> String {
        self.default_source
            .clone()
            .unwrap_or_else(|| syntax::REF_SOURCE_NIXPKGS.to_string())
    }

    /// The named-source resolution table for this pack (D-JPK17).
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

    /// The source-subpath a provided package maps to (R2), if declared.
    pub fn provided(&self, name: &str) -> Option<&str> {
        self.provides
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, sub)| sub.as_str())
    }

    /// Resolve one package entry to a full `<source>:<package>` ref: entries
    /// that already carry a source pass through; bare entries take the default.
    fn entry_ref(&self, entry: &str) -> String {
        if entry.contains(syntax::REF_SEPARATOR) {
            entry.to_string()
        } else {
            format!("{}{}{}", self.source_label(), syntax::REF_SEPARATOR, entry)
        }
    }

    /// Reconstruct the canonical refs this env declares.
    pub fn refs(&self) -> Vec<String> {
        self.packages.iter().map(|p| self.entry_ref(p)).collect()
    }

    /// Render the canonical `pack.jet` text for this environment.
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
        for (name, sub) in &self.provides {
            lines.push(format!("        pkg.package(\"{name}\", \"{sub}\");"));
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
            "// {file} — a Jetpack dev environment (Jet's flake equivalent)\n\
             import jetpack as pkg;\n\
             \n\
             pub fn shell() -> [JSON] {{\n\
             \x20   return [\n\
             {body}\n\
             \x20   ];\n\
             }}\n",
            file = syntax::PACK_FILE,
            body = lines.join("\n"),
        )
    }
}

/// Path to the pack file in a project dir.
pub fn path_in(dir: &Path) -> PathBuf {
    dir.join(syntax::PACK_FILE)
}

/// Load and parse the pack file in `dir`, if present.
pub fn load(dir: &Path) -> Option<PackFile> {
    let text = std::fs::read_to_string(path_in(dir)).ok()?;
    Some(parse(&text))
}

/// Parse the directive calls out of pack-file text. Tolerant: anything it
/// doesn't recognize is ignored. `pkg.source` is read for every occurrence —
/// one arg sets the default source, two declare a named source (D-JPK17).
pub fn parse(text: &str) -> PackFile {
    let mut default_source = None;
    let mut named = Vec::new();
    for call in all_call_args(text, syntax::PACK_DIRECTIVE_SOURCE) {
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
    let mut provides = Vec::new();
    for call in all_call_args(text, syntax::PACK_DIRECTIVE_PACKAGE) {
        if let [name, sub, ..] = quoted_strings(&call).as_slice() {
            provides.push((name.clone(), sub.clone()));
        }
    }
    PackFile {
        default_source,
        named,
        packages: list_arg(text, syntax::PACK_DIRECTIVE_PACKAGES),
        prompt: string_arg(text, syntax::PACK_DIRECTIVE_PROMPT),
        provides,
    }
}

/// How a ref should be stored as a package entry: bare when it matches the
/// default source, otherwise `name:package` so the source survives.
fn entry_for(pf: &PackFile, spec: &RefSpec) -> String {
    let source = spec.source.label();
    let is_default = match &spec.source {
        Source::Named(_) => false,
        builtin => builtin.label() == pf.source_label(),
    };
    if is_default {
        spec.package.clone()
    } else {
        format!("{source}{}{}", syntax::REF_SEPARATOR, spec.package)
    }
}

/// Add a package (from a classified ref) to the pack file in `dir`, creating
/// the file if absent. Returns the updated `PackFile`. Idempotent.
pub fn add(dir: &Path, spec: &RefSpec) -> std::io::Result<PackFile> {
    let mut pf = load(dir).unwrap_or_default();
    // A built-in ref with no default yet becomes the default source.
    if pf.default_source.is_none() && Source::is_builtin(spec.source.label()) {
        pf.default_source = Some(spec.source.label().to_string());
    }
    let entry = entry_for(&pf, spec);
    if !pf.packages.contains(&entry) {
        pf.packages.push(entry);
        pf.packages.sort();
    }
    std::fs::write(path_in(dir), pf.render())?;
    Ok(pf)
}

/// Remove a package from the pack file in `dir`. Matches on the fully-resolved
/// ref, so it works whether the entry was stored bare or `name:package`.
pub fn remove(dir: &Path, spec: &RefSpec) -> std::io::Result<(PackFile, bool)> {
    let mut pf = load(dir).unwrap_or_default();
    let target = format!(
        "{}{}{}",
        spec.source.label(),
        syntax::REF_SEPARATOR,
        spec.package
    );
    let before = pf.packages.len();
    let resolved: Vec<String> = pf.packages.iter().map(|p| pf.entry_ref(p)).collect();
    let mut keep = Vec::new();
    for (entry, full) in pf.packages.iter().zip(resolved.iter()) {
        if full != &target {
            keep.push(entry.clone());
        }
    }
    pf.packages = keep;
    let removed = pf.packages.len() != before;
    if removed {
        std::fs::write(path_in(dir), pf.render())?;
    }
    Ok((pf, removed))
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
    use super::super::refspec::{classify, classify_in};
    use super::*;

    const SAMPLE: &str = r#"
import jetpack as pkg;
pub fn shell() -> [JSON] {
    return [
        pkg.source("nixpkgs");
        pkg.packages(["ripgrep", "fd", "claude-code"]);
        pkg.prompt("jetpack");
    ];
}
"#;

    const NAMED: &str = r#"
import jetpack as pkg;
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
        let pf = parse(SAMPLE);
        assert_eq!(pf.default_source.as_deref(), Some("nixpkgs"));
        assert!(pf.named.is_empty());
        assert_eq!(pf.packages, vec!["ripgrep", "fd", "claude-code"]);
        assert_eq!(pf.prompt.as_deref(), Some("jetpack"));
        assert_eq!(
            pf.refs(),
            vec!["nixpkgs:ripgrep", "nixpkgs:fd", "nixpkgs:claude-code"]
        );
    }

    #[test]
    fn parses_named_sources() {
        let pf = parse(NAMED);
        assert_eq!(pf.named.len(), 2);
        assert_eq!(pf.named[0].name, "stable");
        assert_eq!(pf.named[0].upstream, "github:NixOS/nixpkgs/nixos-24.05");
        assert_eq!(pf.named[0].via, None);
        assert_eq!(pf.named[1].name, "unstable");
        assert_eq!(pf.refs(), vec!["stable:ripgrep", "unstable:neovim"]);
        // Every declared name classifies against the pack's table.
        let table = pf.source_table();
        for r in pf.refs() {
            assert!(classify_in(&r, &table).is_ok(), "ref {r} should classify");
        }
    }

    #[test]
    fn parses_core_source_and_provides() {
        let repo = r#"
import jetpack as pkg;
pub fn shell() -> [JSON] {
    return [
        pkg.source("mine", "path:./jet-pkgs", "core");
        pkg.package("hello", "./pkgs/hello");
        pkg.packages(["mine:hello"]);
    ];
}
"#;
        let pf = parse(repo);
        assert_eq!(pf.named.len(), 1);
        assert_eq!(pf.named[0].via.as_deref(), Some("core"));
        assert_eq!(pf.provided("hello"), Some("./pkgs/hello"));
        // The `core` provider is selected for the named source.
        assert_eq!(
            pf.source_table().provider("mine"),
            super::super::refspec::ProviderKind::Core
        );
        // `pkg.package` is not confused with `pkg.packages`.
        assert_eq!(pf.packages, vec!["mine:hello"]);
    }

    #[test]
    fn render_roundtrips_named() {
        let pf = parse(NAMED);
        let rendered = pf.render();
        assert!(rendered.contains("pub fn shell() -> [JSON]"));
        for line in rendered
            .lines()
            .filter(|line| line.trim_start().starts_with("pkg."))
        {
            assert!(line.trim_end().ends_with(';'), "directive line: {line}");
            assert!(!line.trim_end().ends_with(','), "directive line: {line}");
        }
        let pf2 = parse(&rendered);
        assert_eq!(pf.named, pf2.named);
        assert_eq!(pf.default_source, pf2.default_source);
        assert_eq!(pf.refs(), pf2.refs());
    }

    #[test]
    fn render_roundtrips() {
        let pf = parse(SAMPLE);
        let pf2 = parse(&pf.render());
        assert_eq!(pf.default_source, pf2.default_source);
        assert_eq!(pf.prompt, pf2.prompt);
        let mut a = pf.packages.clone();
        let mut b = pf2.packages.clone();
        a.sort();
        b.sort();
        assert_eq!(a, b);
    }

    #[test]
    fn defaults_when_empty() {
        let pf = PackFile::default();
        assert_eq!(pf.prompt_label(), "jetpack");
        assert_eq!(pf.source_label(), "nixpkgs");
    }

    #[test]
    fn add_creates_and_dedupes() {
        let dir = scratch();
        let pf = add(&dir, &classify("nixpkgs:ripgrep").unwrap()).unwrap();
        assert_eq!(pf.packages, vec!["ripgrep"]);
        // adding again is a no-op
        let pf = add(&dir, &classify("nixpkgs:ripgrep").unwrap()).unwrap();
        assert_eq!(pf.packages, vec!["ripgrep"]);
        let pf = add(&dir, &classify("nixpkgs:fd").unwrap()).unwrap();
        assert_eq!(pf.packages, vec!["fd", "ripgrep"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_edits_file() {
        let dir = scratch();
        add(&dir, &classify("nixpkgs:ripgrep").unwrap()).unwrap();
        add(&dir, &classify("nixpkgs:fd").unwrap()).unwrap();
        let (pf, removed) = remove(&dir, &classify("nixpkgs:fd").unwrap()).unwrap();
        assert!(removed);
        assert_eq!(pf.packages, vec!["ripgrep"]);
        let (_pf, removed) = remove(&dir, &classify("nixpkgs:nope").unwrap()).unwrap();
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
        let pf = add(&dir, &classify_in("unstable:fd", &table).unwrap()).unwrap();
        assert!(pf.packages.contains(&"unstable:fd".to_string()));
        assert_eq!(pf.named.len(), 2, "named sources must survive an edit");
        // Removing it by its full ref works.
        let (pf, removed) = remove(&dir, &classify_in("unstable:fd", &table).unwrap()).unwrap();
        assert!(removed);
        assert!(!pf.packages.contains(&"unstable:fd".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn scratch() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!(
            "jpk-pack-{nanos}-{:?}",
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}
