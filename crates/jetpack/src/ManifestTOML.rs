//! `jetpack.toml` monorepo manifest parser (D-JPK-FILES, ratified 2026-06-18).
//!
//! Parses a TOML subset with two active tables:
//!
//! ```toml
//! [repo]
//! name = "my-repo"
//! version = "0.1.0"
//!
//! [sources]
//! core = "path@./packages"
//! nixpkgs = "github@NixOS/nixpkgs/nixos-24.05"
//! ```
//!
//! A schema layer over the full TOML 1.0 parser in [`super::TOML`] (std-only,
//! I6): the generic parser owns all syntax (every value type, dotted/quoted
//! keys, arrays, inline tables, multi-line strings, comments); this file
//! validates the parsed statements against the active `jetpack.toml`
//! schema. Error model: a TOML syntax error becomes E1214; an unknown table or
//! key becomes E1215 with a did-you-mean suggestion; the retired `[packages]`
//! monorepo index becomes E1225. Parsing continues past errors so the caller
//! sees all problems in one run.

use crate::TOML;
use crate::Syntax;
use crate::Syntax::edit_distance;
use std::collections::HashSet;
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
    /// Retired `[packages]` entries. Kept only for migration diagnostics.
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
                Syntax::JETPACK_TOML,
            ),
        }
    }

    fn e1215(line: usize, kind: &str, name: &str, suggestion: Option<&str>) -> Self {
        let msg = match suggestion {
            Some(s) => format!(
                "`{}` {kind} `{name}` is not recognized. Did you mean `{s}`?",
                Syntax::JETPACK_TOML,
            ),
            None => format!(
                "`{}` {kind} `{name}` is not recognized. \
                 Check the allowed names for this table.",
                Syntax::JETPACK_TOML,
            ),
        };
        TomlError {
            code: "E1215",
            line,
            message: msg,
        }
    }

    fn e1225(line: usize) -> Self {
        TomlError {
            code: "E1225",
            line,
            message: format!(
                "`{}` `[packages]` is retired. Use `{}` with `module workspace {{ members: find(\"./packages\") }}`.",
                Syntax::JETPACK_TOML,
                Syntax::WORKSPACE_FILE,
            ),
        }
    }
}

// ──────────────────────────────────────────────
// Known names (for did-you-mean)
// ──────────────────────────────────────────────

const KNOWN_TABLES: &[&str] = &[Syntax::JTOML_TABLE_REPO, Syntax::JTOML_TABLE_SOURCES];

const KNOWN_REPO_KEYS: &[&str] = &[Syntax::JTOML_KEY_NAME, Syntax::JTOML_KEY_VERSION];

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
/// Runs the full TOML parser, then validates the statements against the
/// three-table schema. Returns `(manifest, errors)`; parsing continues past
/// errors and the manifest holds whatever was valid. A syntax error or an
/// unknown table header puts the parser into a skip state for that section so
/// one bad header does not cascade E1215s across every entry beneath it.
pub fn parse(raw: &str) -> (JetpackToml, Vec<TomlError>) {
    let mut manifest = JetpackToml::default();
    let mut errors: Vec<TomlError> = Vec::new();
    let mut assigned = HashSet::new();
    let mut declared_tables = HashSet::new();
    let mut dotted_tables = HashSet::new();

    let (items, syntax_errors) = TOML::parse(raw);
    for e in syntax_errors {
        errors.push(TomlError::e1214(e.line, &cap_sentence(&e.message)));
    }

    let mut table = Table::None;
    for item in &items {
        match item {
            TOML::Item::Header { path, array, line } => {
                if *array {
                    errors.push(TomlError::e1214(
                        *line,
                        "`[[…]]` array-of-tables are not used in `jetpack.toml`.",
                    ));
                    table = Table::Unknown;
                    continue;
                }
                // An empty path is the parser's marker for a malformed header;
                // E1214 was already recorded, so just enter skip mode.
                if path.is_empty() {
                    table = Table::Unknown;
                    continue;
                }
                let name = path.join(".");
                if !declared_tables.insert(name.clone())
                    || dotted_tables.contains(&name)
                    || assigned.contains(&name)
                {
                    errors.push(TomlError::e1214(
                        *line,
                        &format!("The table `{name}` is defined more than once."),
                    ));
                    table = Table::Unknown;
                    continue;
                }
                table = match path.as_slice() {
                    [t] if t == Syntax::JTOML_TABLE_REPO => Table::Repo,
                    [t] if t == Syntax::JTOML_TABLE_SOURCES => Table::Sources,
                    [t] if t == Syntax::JTOML_TABLE_PACKAGES => {
                        errors.push(TomlError::e1225(*line));
                        Table::Packages
                    }
                    _ => {
                        let suggest = if path.len() == 1 {
                            closest_in(&name, KNOWN_TABLES)
                        } else {
                            None
                        };
                        errors.push(TomlError::e1215(*line, "table", &name, suggest));
                        Table::Unknown
                    }
                };
            }
            TOML::Item::KeyVal { path, value, line } => {
                if table == Table::None && path.len() >= 2 {
                    let full = path.join(".");
                    let prefixes: Vec<String> = (1..path.len())
                        .map(|end| path[..end].join("."))
                        .collect();
                    if dotted_tables.contains(&full)
                        || declared_tables.contains(&full)
                        || prefixes.iter().any(|prefix| assigned.contains(prefix))
                    {
                        errors.push(TomlError::e1214(
                            *line,
                            &format!("The dotted key `{full}` collides with an existing value or table."),
                        ));
                        continue;
                    }
                    dotted_tables.extend(prefixes);
                }
                // Resolve the effective table and key. A dotted top-level key
                // (`repo.name = …`) selects a table by its first segment.
                let (target, key_parts): (Table, &[String]) =
                    if table == Table::None && path.len() >= 2 {
                        match path[0].as_str() {
                            t if t == Syntax::JTOML_TABLE_REPO => (Table::Repo, &path[1..]),
                            t if t == Syntax::JTOML_TABLE_SOURCES => (Table::Sources, &path[1..]),
                            t if t == Syntax::JTOML_TABLE_PACKAGES => {
                                errors.push(TomlError::e1225(*line));
                                (Table::Packages, &path[1..])
                            }
                            _ => (Table::None, &path[..]),
                        }
                    } else {
                        (table, &path[..])
                    };
                let key = key_parts.join(".");

                let qualified = match target {
                    Table::Repo => Some(format!("repo.{key}")),
                    Table::Sources => Some(format!("sources.{key}")),
                    Table::Packages => Some(format!("packages.{key}")),
                    Table::None | Table::Unknown => None,
                };
                if let Some(qualified) = qualified {
                    if !assigned.insert(qualified.clone()) {
                        errors.push(TomlError::e1214(
                            *line,
                            &format!("The key `{qualified}` is assigned more than once."),
                        ));
                        continue;
                    }
                }

                match target {
                    Table::Unknown => {} // header error already reported
                    Table::None => {
                        errors.push(TomlError::e1215(*line, "key", &key, None));
                    }
                    Table::Repo => {
                        if key_parts.len() == 1 && key == Syntax::JTOML_KEY_NAME {
                            match as_string(value, &key, *line) {
                                Ok(s) => manifest.repo.name = Some(s),
                                Err(e) => errors.push(e),
                            }
                        } else if key_parts.len() == 1 && key == Syntax::JTOML_KEY_VERSION {
                            match as_string(value, &key, *line) {
                                Ok(s) => manifest.repo.version = Some(s),
                                Err(e) => errors.push(e),
                            }
                        } else {
                            let suggest = if key_parts.len() == 1 {
                                closest_in(&key, KNOWN_REPO_KEYS)
                            } else {
                                None
                            };
                            errors.push(TomlError::e1215(*line, "key", &key, suggest));
                        }
                    }
                    Table::Sources | Table::Packages => {
                        if key.is_empty() {
                            errors
                                .push(TomlError::e1214(*line, "An entry name must not be empty."));
                            continue;
                        }
                        match as_string(value, &key, *line) {
                            Ok(s) => {
                                if target == Table::Sources {
                                    manifest.sources.push((key, s));
                                } else {
                                    manifest.packages.push((key, s));
                                }
                            }
                            Err(e) => errors.push(e),
                        }
                    }
                }
            }
        }
    }

    errors.sort_by_key(|e| e.line);
    (manifest, errors)
}

/// A `jetpack.toml` value field must be a string. Anything else (number, bool,
/// array, …) is an E1214 with the offending key named.
fn as_string(value: &TOML::Value, key: &str, line: usize) -> Result<String, TomlError> {
    match value {
        TOML::Value::String(s) => Ok(s.clone()),
        _ => Err(TomlError::e1214(
            line,
            &format!("The value for `{key}` must be a quoted string."),
        )),
    }
}

/// Capitalize the first letter and ensure a trailing period — turns a terse
/// parser message into a diagnostic-voice sentence for the E1214 detail.
fn cap_sentence(s: &str) -> String {
    let mut chars = s.chars();
    let mut out = match chars.next() {
        Some(f) => f.to_uppercase().collect::<String>(),
        None => return String::new(),
    };
    out.push_str(chars.as_str());
    if !out.ends_with('.') {
        out.push('.');
    }
    out
}

/// Load and parse `jetpack.toml` from `dir`. Returns `None` if the file
/// doesn't exist (that is not an error — a plain package repo has no
/// `jetpack.toml`).
pub fn load(dir: &Path) -> Option<(JetpackToml, Vec<TomlError>)> {
    let path = dir.join(Syntax::JETPACK_TOML);
    let raw = std::fs::read_to_string(&path).ok()?;
    Some(parse(&raw))
}

// ──────────────────────────────────────────────
// Render (E1214/E1215 → string matching diagnostics.md voice)
// ──────────────────────────────────────────────

/// Render a list of `TomlError`s as a multi-line string using the standard
/// Jet diagnostic format (spanless — no source file context block).
pub fn render_errors(_path: &str, errors: &[TomlError]) -> String {
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
        let raw =
            "[sources]\ncore = \"path@./pkgs\"\nnixpkgs = \"github@NixOS/nixpkgs/nixos-24.05\"\n";
        let m = ok(raw);
        assert_eq!(m.sources.len(), 2);
        assert_eq!(
            m.sources[0],
            ("core".to_string(), "path@./pkgs".to_string())
        );
        assert_eq!(
            m.sources[1],
            (
                "nixpkgs".to_string(),
                "github@NixOS/nixpkgs/nixos-24.05".to_string()
            )
        );
    }

    #[test]
    fn parse_packages_table() {
        let raw = "[packages]\nhello = \"packages/hello/pkg.jet\"\n";
        let es = errs(raw);
        assert_eq!(es.len(), 1);
        assert_eq!(es[0].code, "E1225");
    }

    #[test]
    fn parse_full_manifest() {
        let raw = "\
[repo]
name = \"acme\"
version = \"0.1.0\"

[sources]
core = \"path@./packages\"

";
        let (m, es) = parse(raw);
        assert!(es.is_empty(), "unexpected errors: {es:?}");
        assert_eq!(m.repo.name.as_deref(), Some("acme"));
        assert_eq!(m.sources[0].0, "core");
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
        assert!(
            es[0].message.contains("name"),
            "expected did-you-mean `name` in: {}",
            es[0].message
        );
    }

    #[test]
    fn e1215_unknown_repo_key_no_suggestion() {
        let es = errs("[repo]\nzxqwvbn = \"x\"\n");
        assert_eq!(es.len(), 1);
        assert_eq!(es[0].code, "E1215");
        assert!(
            !es[0].message.contains("Did you mean"),
            "no suggestion expected"
        );
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
or table header. Expected `=` after key `bad`.\n";
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
    fn render_e1225_snapshot() {
        // I4: pin the rendered migration diagnostic for the retired monorepo
        // index.
        let path = "jetpack.toml";
        let raw = "[packages]\nhello = \"packages/hello/pkg.jet\"\n";
        let (_, errors) = parse(raw);
        let rendered = render_errors(path, &errors);
        let expected = "Error [E1225]: `jetpack.toml` `[packages]` is retired. Use `workspace.jet` with `module workspace { members: find(\"./packages\") }`.\n";
        assert_eq!(rendered, expected);
    }

    #[test]
    fn full_toml_value_types_parse() {
        // The schema only stores strings, but the full parser accepts every
        // TOML construct under the recognized tables (escapes here prove the
        // string value is unquoted + unescaped, not taken verbatim).
        let m = ok("[repo]\nname = \"a\\tb\"\nversion = \"0.1.0\"\n");
        assert_eq!(m.repo.name.as_deref(), Some("a\tb"));
        assert_eq!(m.repo.version.as_deref(), Some("0.1.0"));
    }

    #[test]
    fn dotted_top_level_key_selects_table() {
        let m = ok("repo.name = \"acme\"\n");
        assert_eq!(m.repo.name.as_deref(), Some("acme"));
    }

    #[test]
    fn non_string_value_is_e1214() {
        // `version = 1` is valid TOML (an integer) but the schema wants a string.
        let es = errs("[repo]\nversion = 1\n");
        assert_eq!(es.len(), 1);
        assert_eq!(es[0].code, "E1214");
        assert!(
            es[0].message.contains("must be a quoted string"),
            "{}",
            es[0].message
        );
    }

    #[test]
    fn duplicate_keys_are_rejected_instead_of_silently_overwritten() {
        let (manifest, errors) = parse(
            "[repo]\nname = \"first\"\nname = \"second\"\n[sources]\ncore = \"a\"\ncore = \"b\"\n",
        );
        assert_eq!(manifest.repo.name.as_deref(), Some("first"));
        assert_eq!(manifest.sources, vec![("core".into(), "a".into())]);
        assert_eq!(errors.len(), 2);
        assert!(errors.iter().all(|error| error.code == "E1214"));
    }

    #[test]
    fn duplicate_tables_and_dotted_table_collisions_are_rejected() {
        for raw in [
            "[repo]\nname = \"first\"\n[repo]\nversion = \"1.0.0\"\n",
            "repo.name = \"first\"\n[repo]\nversion = \"1.0.0\"\n",
            "repo.meta.value = \"first\"\nrepo.meta = \"second\"\n",
        ] {
            let errors = errs(raw);
            assert!(
                errors.iter().any(|error| error.code == "E1214"),
                "accepted table collision: {raw}"
            );
        }
    }
}
