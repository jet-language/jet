//! `jetpack.toml` monorepo manifest parser (D-JPK-FILES, ratified 2026-06-18).
//!
//! Parses a TOML subset with three tables:
//!
//! ```toml
//! [repo]
//! name = "my-repo"
//! version = "0.1.0"
//!
//! [sources]
//! core = "path@./packages"
//! nixpkgs = "github@NixOS/nixpkgs/nixos-24.05"
//!
//! [packages]
//! wordstats = "packages/wordstats/pkg.jet"
//! hello = "packages/hello/pkg.jet"
//! ```
//!
//! Hand-written std-only (I6). Error model: every malformed line emits E1214;
//! every unknown table/key emits E1215 with a did-you-mean suggestion. Parsing
//! continues past errors so the caller sees all problems in one run.

use crate::cli::edit_distance;
use crate::syntax;
use std::path::Path;

// ──────────────────────────────────────────────
// Public types
// ──────────────────────────────────────────────

/// A successfully parsed `jetpack.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JetpackToml {
    /// `[repo]` table fields.
    pub repo: RepoMeta,
    /// `[sources]` table: `name → provider@target[#ver]` ref strings.
    pub sources: Vec<(String, String)>,
    /// `[packages]` table: `name → relative/path/pkg.jet` (optional explicit
    /// index; discovery falls back to `find . -name pkg.jet`).
    pub packages: Vec<(String, String)>,
}

/// Fields from the `[repo]` table.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RepoMeta {
    pub name: Option<String>,
    pub version: Option<String>,
}

/// A parse error — E1214 (malformed line) or E1215 (unknown name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TomlError {
    pub code: &'static str,
    pub line: usize,
    pub message: String,
}

impl TomlError {
    fn e1214(line: usize, detail: &str) -> Self {
        TomlError {
            code: "E1214",
            line,
            message: format!(
                "`{}` line {line} is not a valid assignment or table header. {detail}",
                syntax::JETPACK_TOML,
            ),
        }
    }

    fn e1215(line: usize, kind: &str, name: &str, suggestion: Option<&str>) -> Self {
        let msg = match suggestion {
            Some(s) => format!(
                "`{}` {kind} `{name}` is not recognized. Did you mean `{s}`?",
                syntax::JETPACK_TOML,
            ),
            None => format!(
                "`{}` {kind} `{name}` is not recognized. \
                 Check the allowed names for this table.",
                syntax::JETPACK_TOML,
            ),
        };
        TomlError {
            code: "E1215",
            line,
            message: msg,
        }
    }
}

// ──────────────────────────────────────────────
// Known names (for did-you-mean)
// ──────────────────────────────────────────────

const KNOWN_TABLES: &[&str] = &[
    syntax::JTOML_TABLE_REPO,
    syntax::JTOML_TABLE_SOURCES,
    syntax::JTOML_TABLE_PACKAGES,
];

const KNOWN_REPO_KEYS: &[&str] = &[syntax::JTOML_KEY_NAME, syntax::JTOML_KEY_VERSION];

fn closest_in(needle: &str, candidates: &[&'static str]) -> Option<&'static str> {
    let mut best: Option<(&'static str, usize)> = None;
    for &cand in candidates {
        let d = edit_distance(needle, cand);
        if best.map(|(_, bd)| d < bd).unwrap_or(true) {
            best = Some((cand, d));
        }
    }
    // Only suggest when clearly close: within 3 edits and at most half the candidate length.
    best.filter(|(c, d)| *d <= 3 && *d <= c.len() / 2 + 1)
        .map(|(c, _)| c)
}

// ──────────────────────────────────────────────
// Parser
// ──────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Table {
    None,
    Repo,
    Sources,
    Packages,
    Unknown,
}

/// Parse the content of a `jetpack.toml` file.
///
/// Returns `(manifest, errors)`. Parsing continues past errors; the manifest
/// holds whatever was valid. A `Table::Unknown` section silently skips all its
/// key/value lines to avoid cascading E1215s for every entry under a bad table.
pub fn parse(raw: &str) -> (JetpackToml, Vec<TomlError>) {
    let mut manifest = JetpackToml::default();
    let mut errors: Vec<TomlError> = Vec::new();
    let mut table = Table::None;

    for (i, raw_line) in raw.lines().enumerate() {
        let lineno = i + 1;
        let line = raw_line.trim();

        // Skip blank lines and comments.
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Table header: `[name]`
        if line.starts_with('[') {
            if !line.ends_with(']') {
                errors.push(TomlError::e1214(
                    lineno,
                    "A table header must end with `]`.",
                ));
                table = Table::Unknown;
                continue;
            }
            // Reject array-of-tables `[[…]]`.
            if line.starts_with("[[") {
                errors.push(TomlError::e1214(
                    lineno,
                    "`[[array]]` tables are not used in `jetpack.toml`.",
                ));
                table = Table::Unknown;
                continue;
            }
            let name = line[1..line.len() - 1].trim();
            table = match name {
                t if t == syntax::JTOML_TABLE_REPO => Table::Repo,
                t if t == syntax::JTOML_TABLE_SOURCES => Table::Sources,
                t if t == syntax::JTOML_TABLE_PACKAGES => Table::Packages,
                unknown => {
                    let suggest = closest_in(unknown, KNOWN_TABLES);
                    errors.push(TomlError::e1215(lineno, "table", unknown, suggest));
                    Table::Unknown
                }
            };
            continue;
        }

        // Key = value line.
        let Some((key_raw, val_raw)) = line.split_once('=') else {
            errors.push(TomlError::e1214(
                lineno,
                "Expected `key = \"value\"` or a `[table]` header.",
            ));
            continue;
        };
        let key = key_raw.trim();
        let val = unquote(val_raw.trim());

        // Skip keys under unknown tables — we already emitted E1215 for the header.
        if table == Table::Unknown {
            continue;
        }

        // Top-level key before any table header.
        if table == Table::None {
            errors.push(TomlError::e1215(
                lineno,
                "key",
                key,
                None,
            ));
            continue;
        }

        match table {
            Table::Repo => match key {
                k if k == syntax::JTOML_KEY_NAME => manifest.repo.name = Some(val),
                k if k == syntax::JTOML_KEY_VERSION => manifest.repo.version = Some(val),
                unknown => {
                    let suggest = closest_in(unknown, KNOWN_REPO_KEYS);
                    errors.push(TomlError::e1215(lineno, "key", unknown, suggest));
                }
            },
            Table::Sources => {
                // Any key is a source name — no fixed vocabulary here.
                if key.is_empty() {
                    errors.push(TomlError::e1214(lineno, "Source name must not be empty."));
                } else {
                    manifest.sources.push((key.to_string(), val));
                }
            }
            Table::Packages => {
                // Any key is a package name.
                if key.is_empty() {
                    errors.push(TomlError::e1214(lineno, "Package name must not be empty."));
                } else {
                    manifest.packages.push((key.to_string(), val));
                }
            }
            Table::None | Table::Unknown => unreachable!(),
        }
    }

    (manifest, errors)
}

/// Strip surrounding `"…"` from a TOML bare string value.
/// If the value has no quotes, return it as-is (tolerates bare values).
fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        s[1..s.len() - 1].replace("\\\"", "\"").replace("\\\\", "\\")
    } else {
        s.to_string()
    }
}

/// Load and parse `jetpack.toml` from `dir`. Returns `None` if the file
/// doesn't exist (that is not an error — a plain package repo has no
/// `jetpack.toml`).
pub fn load(dir: &Path) -> Option<(JetpackToml, Vec<TomlError>)> {
    let path = dir.join(syntax::JETPACK_TOML);
    let raw = std::fs::read_to_string(&path).ok()?;
    Some(parse(&raw))
}

// ──────────────────────────────────────────────
// Render (E1214/E1215 → string matching diagnostics.md voice)
// ──────────────────────────────────────────────

/// Render a list of `TomlError`s as a multi-line string using the standard
/// Jet diagnostic format (spanless — no source file context block).
pub fn render_errors(path: &str, errors: &[TomlError]) -> String {
    errors
        .iter()
        .map(|e| format!("Error [{}]: {}\n", e.code, e.message))
        .collect::<Vec<_>>()
        .join("\n")
}

// ──────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(raw: &str) -> JetpackToml {
        let (manifest, errors) = parse(raw);
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        manifest
    }

    fn errs(raw: &str) -> Vec<TomlError> {
        let (_, errors) = parse(raw);
        errors
    }

    #[test]
    fn parse_empty() {
        assert_eq!(ok(""), JetpackToml::default());
    }

    #[test]
    fn parse_comments_and_blanks() {
        let raw = "# this is a comment\n\n# another\n";
        assert_eq!(ok(raw), JetpackToml::default());
    }

    #[test]
    fn parse_repo_table() {
        let raw = "[repo]\nname = \"my-repo\"\nversion = \"0.2.0\"\n";
        let m = ok(raw);
        assert_eq!(m.repo.name.as_deref(), Some("my-repo"));
        assert_eq!(m.repo.version.as_deref(), Some("0.2.0"));
    }

    #[test]
    fn parse_sources_table() {
        let raw = "[sources]\ncore = \"path@./pkgs\"\nnixpkgs = \"github@NixOS/nixpkgs/nixos-24.05\"\n";
        let m = ok(raw);
        assert_eq!(m.sources.len(), 2);
        assert_eq!(m.sources[0], ("core".to_string(), "path@./pkgs".to_string()));
        assert_eq!(
            m.sources[1],
            ("nixpkgs".to_string(), "github@NixOS/nixpkgs/nixos-24.05".to_string())
        );
    }

    #[test]
    fn parse_packages_table() {
        let raw = "[packages]\nhello = \"packages/hello/pkg.jet\"\n";
        let m = ok(raw);
        assert_eq!(m.packages.len(), 1);
        assert_eq!(
            m.packages[0],
            ("hello".to_string(), "packages/hello/pkg.jet".to_string())
        );
    }

    #[test]
    fn parse_full_manifest() {
        let raw = "\
[repo]
name = \"acme\"
version = \"0.1.0\"

[sources]
core = \"path@./packages\"

[packages]
wordstats = \"packages/wordstats/pkg.jet\"
";
        let m = ok(raw);
        assert_eq!(m.repo.name.as_deref(), Some("acme"));
        assert_eq!(m.sources[0].0, "core");
        assert_eq!(m.packages[0].0, "wordstats");
    }

    #[test]
    fn e1214_malformed_line() {
        let es = errs("[repo]\nthis is not valid\n");
        assert_eq!(es.len(), 1);
        assert_eq!(es[0].code, "E1214");
        assert_eq!(es[0].line, 2);
    }

    #[test]
    fn e1214_unclosed_table_header() {
        let es = errs("[repo\nname = \"x\"\n");
        assert_eq!(es.len(), 1);
        assert_eq!(es[0].code, "E1214");
    }

    #[test]
    fn e1214_array_table() {
        let es = errs("[[packages]]\nfoo = \"bar\"\n");
        assert_eq!(es.len(), 1);
        assert_eq!(es[0].code, "E1214");
    }

    #[test]
    fn e1215_unknown_table() {
        let es = errs("[workspace]\nfoo = \"bar\"\n");
        assert_eq!(es.len(), 1);
        assert_eq!(es[0].code, "E1215");
        assert_eq!(es[0].line, 1);
        // Should not cascade a second error for the key under the unknown table.
        assert_eq!(es.len(), 1);
    }

    #[test]
    fn e1215_unknown_repo_key() {
        let es = errs("[repo]\nnmae = \"typo\"\n");
        assert_eq!(es.len(), 1);
        assert_eq!(es[0].code, "E1215");
        // "nmae" is close to "name" — suggestion expected.
        assert!(es[0].message.contains("name"), "expected did-you-mean `name` in: {}", es[0].message);
    }

    #[test]
    fn e1215_unknown_repo_key_no_suggestion() {
        let es = errs("[repo]\nzxqwvbn = \"x\"\n");
        assert_eq!(es.len(), 1);
        assert_eq!(es[0].code, "E1215");
        assert!(!es[0].message.contains("Did you mean"), "no suggestion expected");
    }

    #[test]
    fn multiple_errors_continue_parsing() {
        let raw = "[repo]\nnmae = \"a\"\nbadline\nname = \"b\"\n";
        let (m, es) = parse(raw);
        // E1215 for "nmae", E1214 for "badline"
        assert_eq!(es.len(), 2);
        // Valid "name = b" line still parsed.
        assert_eq!(m.repo.name.as_deref(), Some("b"));
    }

    #[test]
    fn render_e1214_snapshot() {
        // I4: pin the rendered form here; the ui harness only renders front-end
        // `.jet` diagnostics — `jetpack.toml` errors are a CLI concern.
        let path = "jetpack.toml";
        let raw = "[repo]\nbad line here\n";
        let (_, errors) = parse(raw);
        let rendered = render_errors(path, &errors);
        let expected = "Error [E1214]: `jetpack.toml` line 2 is not a valid assignment \
or table header. Expected `key = \"value\"` or a `[table]` header.\n";
        assert_eq!(rendered, expected);
    }

    #[test]
    fn render_e1215_snapshot() {
        // I4: pin the rendered form — see note in render_e1214_snapshot.
        let path = "jetpack.toml";
        let raw = "[workspace]\n";
        let (_, errors) = parse(raw);
        let rendered = render_errors(path, &errors);
        // "workspace" is not close to any known table name — no suggestion.
        let expected = "Error [E1215]: `jetpack.toml` table `workspace` is not recognized. \
Check the allowed names for this table.\n";
        assert_eq!(rendered, expected);
    }

    #[test]
    fn unquote_strips_quotes() {
        assert_eq!(unquote("\"hello\""), "hello");
        assert_eq!(unquote("hello"), "hello");
        assert_eq!(unquote("\"a \\\"b\\\" c\""), "a \"b\" c");
    }
}
