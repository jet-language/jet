//! `jet.toml` hand-parser and validator (M12.1, S52, I6 — no TOML crate).
//!
//! Supports the ratified subset: `[package]`, `[dependencies]`,
//! `[dependencies:rust]`, `[dependencies:c]` (reserved empty), reserved
//! sections that must remain empty, and `[tool.*]` (silently ignored).
//! All other tables are unknown-key errors.

use crate::diag::Diagnostic;
use std::collections::BTreeMap;
use std::path::Path;

/// The compiler's version string for E1208 toolchain checks.
pub const COMPILER_VERSION: &str = env!("CARGO_PKG_VERSION");

// ──────────────────────────────────────────────
// Public types
// ──────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub package: PackageMeta,
    /// Jet package dependencies.
    pub dependencies: BTreeMap<String, DepSpec>,
    /// `[dependencies:rust]` — crate name → version string.
    pub dependencies_rust: BTreeMap<String, String>,
    /// Raw TOML text (preserved for comment-preserving edits).
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageMeta {
    pub name: String,
    pub version: String,
    /// Toolchain constraint from `jet = "..."`.
    pub jet_constraint: Option<String>,
    pub description: Option<String>,
    pub license: Option<String>,
    pub repository: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepSpec {
    /// Path dependency: `helpers = { path = "../helpers" }`.
    Path { path: String },
    /// Git dependency with one selector.
    Git { url: String, selector: GitSelector },
    /// Registry version string (M12.2 only; error in M12.1 during resolution).
    Registry(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitSelector {
    Tag(String),
    Branch(String),
    Rev(String),
}

impl GitSelector {
    pub fn is_moving(&self) -> bool {
        match self {
            GitSelector::Tag(t) => t == "@latest",
            GitSelector::Branch(_) => true,
            GitSelector::Rev(_) => false,
        }
    }
}

// ──────────────────────────────────────────────
// Parser
// ──────────────────────────────────────────────

pub fn parse(path: &Path, raw: &str) -> Result<Manifest, Diagnostic> {
    let file = path.display().to_string();
    let mut parser = TomlParser::new(raw, &file);
    parser.parse()
}

/// Load and parse a `jet.toml` from a directory.
pub fn load(dir: &Path) -> Option<Result<Manifest, Diagnostic>> {
    let toml_path = dir.join("jet.toml");
    if !toml_path.is_file() {
        return None;
    }
    let raw = match std::fs::read_to_string(&toml_path) {
        Ok(s) => s,
        Err(e) => {
            return Some(Err(e1206(
                &toml_path.display().to_string(),
                0,
                &format!("couldn't read jet.toml: {}", e),
            )));
        }
    };
    Some(parse(&toml_path, &raw))
}

/// Validate the toolchain constraint from `[package].jet`. Returns E1208 on mismatch.
pub fn check_toolchain(manifest: &Manifest, _file: &str) -> Result<(), Diagnostic> {
    let Some(constraint) = &manifest.package.jet_constraint else {
        return Ok(());
    };
    if !satisfies_constraint(COMPILER_VERSION, constraint) {
        return Err(Diagnostic::error(
            "E1208",
            format!(
                "this project requires Jet `{}` but this is Jet {}",
                constraint, COMPILER_VERSION
            ),
            "the `jet` field in `[package]` specifies a minimum toolchain version".to_string(),
            "update Jet to a newer version, or change the `jet` field in `jet.toml`".to_string(),
            None,
        ));
    }
    Ok(())
}

fn satisfies_constraint(version: &str, constraint: &str) -> bool {
    let constraint = constraint.trim();
    // Support: ">=X.Y.Z", "^X.Y.Z", "X.Y.Z" (exact), "*".
    if constraint == "*" || constraint.is_empty() {
        return true;
    }
    if let Some(min) = constraint.strip_prefix(">=") {
        return version_ge(version, min.trim());
    }
    if let Some(min) = constraint.strip_prefix("^") {
        // ^X.Y.Z means >=X.Y.Z, <(X+1).0.0
        return version_ge(version, min.trim());
    }
    // Exact match
    version.trim() == constraint
}

fn version_ge(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> (u64, u64, u64) {
        let mut p = s.trim().splitn(3, '.');
        let major = p.next().and_then(|x| x.parse().ok()).unwrap_or(0);
        let minor = p.next().and_then(|x| x.parse().ok()).unwrap_or(0);
        let patch = p.next().and_then(|x| x.parse().ok()).unwrap_or(0);
        (major, minor, patch)
    };
    parse(a) >= parse(b)
}

// ──────────────────────────────────────────────
// Comment-preserving edit helpers for `jet add`/`jet remove`
// ──────────────────────────────────────────────

/// Insert or update a dependency in the `[dependencies]` table, preserving
/// comments and existing entries. Returns the updated TOML text.
pub fn add_dependency(raw: &str, name: &str, spec: &DepSpec) -> String {
    let spec_str = dep_spec_to_toml(spec);
    let line = format!("{} = {}", name, spec_str);
    insert_or_replace_in_table(raw, "dependencies", name, &line)
}

/// Remove a dependency from `[dependencies]`, preserving comments.
pub fn remove_dependency(raw: &str, name: &str) -> String {
    remove_from_table(raw, "dependencies", name)
}

/// Insert or update a dep in `[dependencies:rust]`.
pub fn add_rust_dependency(raw: &str, name: &str, version: &str) -> String {
    let line = format!("{} = \"{}\"", name, version);
    insert_or_replace_in_table(raw, "dependencies:rust", name, &line)
}

fn dep_spec_to_toml(spec: &DepSpec) -> String {
    match spec {
        DepSpec::Path { path } => format!("{{ path = \"{}\" }}", path),
        DepSpec::Git { url, selector } => {
            let sel = match selector {
                GitSelector::Tag(t) => format!(", tag = \"{}\"", t),
                GitSelector::Branch(b) => format!(", branch = \"{}\"", b),
                GitSelector::Rev(r) => format!(", rev = \"{}\"", r),
            };
            format!("{{ git = \"{}\"{}  }}", url, sel)
        }
        DepSpec::Registry(v) => format!("\"{}\"", v),
    }
}

fn insert_or_replace_in_table(raw: &str, table: &str, key: &str, new_line: &str) -> String {
    let header = format!("[{}]", table);
    let lines: Vec<&str> = raw.lines().collect();

    // Find the target table's line range.
    let mut table_start: Option<usize> = None;
    let mut table_end: Option<usize> = None;
    let mut existing_key_line: Option<usize> = None;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed == header {
            table_start = Some(i + 1);
            continue;
        }
        if let Some(start) = table_start {
            if i >= start {
                if trimmed.starts_with('[') && !trimmed.starts_with("[[") {
                    if table_end.is_none() {
                        table_end = Some(i);
                    }
                }
                if table_end.is_none() {
                    let lkey = trimmed.split('=').next().unwrap_or("").trim();
                    if lkey == key {
                        existing_key_line = Some(i);
                    }
                }
            }
        }
    }

    let mut out_lines: Vec<String> = lines.iter().map(|l| l.to_string()).collect();

    if let Some(existing) = existing_key_line {
        // Replace the existing line.
        out_lines[existing] = new_line.to_string();
    } else if let Some(start) = table_start {
        let end = table_end.unwrap_or(lines.len());
        // Insert before the first blank line at the end, or just before table_end.
        let insert_at = (start..end)
            .rev()
            .find(|&i| !lines[i].trim().is_empty())
            .map(|i| i + 1)
            .unwrap_or(end);
        out_lines.insert(insert_at, new_line.to_string());
    } else {
        // Table doesn't exist — append it.
        if !raw.ends_with('\n') {
            out_lines.push(String::new());
        }
        out_lines.push(header);
        out_lines.push(new_line.to_string());
    }

    let mut result = out_lines.join("\n");
    if raw.ends_with('\n') && !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

fn remove_from_table(raw: &str, table: &str, key: &str) -> String {
    let header = format!("[{}]", table);
    let lines: Vec<&str> = raw.lines().collect();
    let mut in_table = false;
    let mut out: Vec<&str> = Vec::new();

    for line in &lines {
        let trimmed = line.trim();
        if trimmed == header {
            in_table = true;
            out.push(line);
            continue;
        }
        if trimmed.starts_with('[') && !trimmed.starts_with("[[") {
            in_table = false;
        }
        if in_table {
            let lkey = trimmed.split('=').next().unwrap_or("").trim();
            if lkey == key {
                continue; // drop this line
            }
        }
        out.push(line);
    }

    let mut result = out.join("\n");
    if raw.ends_with('\n') && !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

/// Generate a `jet.toml` template for `jet new`.
pub fn new_template(name: &str, annotated: bool) -> String {
    let ver = COMPILER_VERSION;
    if annotated {
        format!(
            r#"[package]
name = "{name}"
version = "0.1.0"
jet = ">={ver}"
description = ""
license = "MIT OR Apache-2.0"
repository = ""

# Jet package dependencies:
# [dependencies]
# helpers = {{ path = "../helpers" }}
# parsekit = {{ git = "https://github.com/acme/parsekit", tag = "v0.4.1" }}

# Rust crate dependencies (for extern rust blocks):
# [dependencies:rust]
# base64 = "0.22"
"#
        )
    } else {
        format!(
            r#"[package]
name = "{name}"
version = "0.1.0"
jet = ">={ver}"
description = ""
license = "MIT OR Apache-2.0"
repository = ""

[dependencies]
"#
        )
    }
}

// ──────────────────────────────────────────────
// Tiny TOML hand-parser
// ──────────────────────────────────────────────

struct TomlParser<'a> {
    src: &'a str,
    file: &'a str,
    pos: usize,
    line: usize,
}

impl<'a> TomlParser<'a> {
    fn new(src: &'a str, file: &'a str) -> Self {
        TomlParser {
            src,
            file,
            pos: 0,
            line: 1,
        }
    }

    fn parse(&mut self) -> Result<Manifest, Diagnostic> {
        let raw = self.src.to_string();
        let mut current_table: Option<String> = None;

        let mut pkg_name: Option<String> = None;
        let mut pkg_version: Option<String> = None;
        let mut pkg_jet: Option<String> = None;
        let mut pkg_desc: Option<String> = None;
        let mut pkg_license: Option<String> = None;
        let mut pkg_repo: Option<String> = None;

        let mut deps: BTreeMap<String, DepSpec> = BTreeMap::new();
        let mut deps_rust: BTreeMap<String, String> = BTreeMap::new();

        // Reserved sections that must stay empty.
        let reserved = ["dev-dependencies", "patch", "workspace"];

        while self.pos < self.src.len() {
            self.skip_whitespace_and_comments();
            if self.pos >= self.src.len() {
                break;
            }

            let ch = self.cur_char();

            if ch == '[' {
                // Table header: [section] or [[array-table]] (array tables not supported)
                let line_no = self.line;
                self.advance();
                if self.pos < self.src.len() && self.cur_char() == '[' {
                    return Err(self.e1206(
                        line_no,
                        "array tables `[[…]]` are not supported in jet.toml",
                    ));
                }
                let table_name = self.read_until_char(']')?;
                self.expect_char(']', "expected `]` to close table header")?;
                self.skip_to_eol();

                let table_name = table_name.trim().to_string();

                // Tool tables are silently ignored (except [tool.jet] which warns — not implemented here).
                if table_name.starts_with("tool.") {
                    current_table = Some(format!("_ignore_{}", table_name));
                    continue;
                }

                // Reserved sections: track but validate non-empty below.
                current_table = Some(table_name);
                continue;
            }

            // Key = value line
            let line_no = self.line;
            let key = self.read_key()?;
            if key.is_empty() {
                self.skip_to_eol();
                continue;
            }
            self.skip_inline_ws();
            self.expect_char('=', "expected `=` after key name")?;
            self.skip_inline_ws();

            let table = current_table.as_deref().unwrap_or("");

            // Silently ignore [tool.*] sections.
            if table.starts_with("_ignore_") {
                self.skip_to_eol();
                continue;
            }

            match table {
                "package" => {
                    let val = self.read_string_value()?;
                    match key.as_str() {
                        "name" => pkg_name = Some(val),
                        "version" => pkg_version = Some(val),
                        "jet" => pkg_jet = Some(val),
                        "description" => pkg_desc = Some(val),
                        "license" => pkg_license = Some(val),
                        "repository" => pkg_repo = Some(val),
                        _ => {
                            // Unknown package key — skip with no error (future compat).
                            self.skip_to_eol();
                            continue;
                        }
                    }
                    self.skip_to_eol();
                }
                "dependencies" => {
                    let spec = self.read_dep_spec(&key)?;
                    deps.insert(key, spec);
                }
                "dependencies:rust" => {
                    let val = self.read_string_value()?;
                    deps_rust.insert(key, val);
                    self.skip_to_eol();
                }
                "dependencies:c" => {
                    // Reserved for v2; parse but ignore values.
                    self.skip_to_eol();
                }
                s if reserved.contains(&s) => {
                    // Any key in a reserved section is E1209.
                    return Err(e1209(self.file, s));
                }
                "" => {
                    // Top-level key outside any table — syntax error.
                    return Err(self.e1206(line_no, "key without a table header"));
                }
                _ => {
                    // Unknown table (not tool.*) — skip line.
                    self.skip_to_eol();
                }
            }
        }

        // Validate required [package] fields.
        let name = pkg_name
            .ok_or_else(|| self.e1206(1, "missing required field `name` in `[package]`"))?;
        let version = pkg_version
            .ok_or_else(|| self.e1206(1, "missing required field `version` in `[package]`"))?;

        Ok(Manifest {
            package: PackageMeta {
                name,
                version,
                jet_constraint: pkg_jet,
                description: pkg_desc,
                license: pkg_license,
                repository: pkg_repo,
            },
            dependencies: deps,
            dependencies_rust: deps_rust,
            raw,
        })
    }

    fn read_dep_spec(&mut self, key: &str) -> Result<DepSpec, Diagnostic> {
        let ch = self.cur_char();
        if ch == '"' {
            // Registry string version.
            let val = self.read_string_value()?;
            self.skip_to_eol();
            return Ok(DepSpec::Registry(val));
        }
        if ch == '{' {
            // Inline table.
            self.advance(); // consume '{'
            let mut fields: BTreeMap<String, String> = BTreeMap::new();
            loop {
                self.skip_inline_ws();
                if self.pos >= self.src.len() {
                    return Err(self.e1206(self.line, "unclosed inline table `{`"));
                }
                if self.cur_char() == '}' {
                    self.advance();
                    break;
                }
                let field_key = self.read_key()?;
                if field_key.is_empty() {
                    return Err(self.e1206(self.line, "expected field name in inline table"));
                }
                self.skip_inline_ws();
                self.expect_char('=', "expected `=` in inline table field")?;
                self.skip_inline_ws();
                let field_val = self.read_string_value()?;
                fields.insert(field_key, field_val);
                self.skip_inline_ws();
                if self.pos < self.src.len() && self.cur_char() == ',' {
                    self.advance();
                }
            }
            self.skip_to_eol();

            // Interpret the inline table.
            if let Some(path) = fields.remove("path") {
                return Ok(DepSpec::Path { path });
            }
            if let Some(url) = fields.remove("git") {
                let selector = if let Some(tag) = fields.remove("tag") {
                    GitSelector::Tag(tag)
                } else if let Some(branch) = fields.remove("branch") {
                    GitSelector::Branch(branch)
                } else if let Some(rev) = fields.remove("rev") {
                    GitSelector::Rev(rev)
                } else {
                    return Err(self.e1206(
                        self.line,
                        &format!(
                            "git dependency `{}` must have one of: tag, branch, rev",
                            key
                        ),
                    ));
                };
                return Ok(DepSpec::Git { url, selector });
            }

            return Err(self.e1206(
                self.line,
                &format!("dependency `{}` must have a `path` or `git` field", key),
            ));
        }

        Err(self.e1206(
            self.line,
            &format!("expected a string or inline table for dependency `{}`", key),
        ))
    }

    fn read_key(&mut self) -> Result<String, Diagnostic> {
        self.skip_inline_ws();
        let start = self.pos;
        while self.pos < self.src.len() {
            let c = self.cur_char();
            if c.is_alphanumeric() || c == '_' || c == '-' || c == ':' {
                self.advance_char(c);
            } else {
                break;
            }
        }
        Ok(self.src[start..self.pos].to_string())
    }

    fn read_string_value(&mut self) -> Result<String, Diagnostic> {
        let line_no = self.line;
        if self.pos >= self.src.len() || self.cur_char() != '"' {
            return Err(self.e1206(line_no, "expected a quoted string value"));
        }
        self.advance(); // opening "
        let mut s = String::new();
        loop {
            if self.pos >= self.src.len() {
                return Err(self.e1206(line_no, "unclosed string literal"));
            }
            let c = self.cur_char();
            if c == '"' {
                self.advance();
                return Ok(s);
            }
            if c == '\n' {
                return Err(self.e1206(line_no, "string literal must fit on one line"));
            }
            if c == '\\' {
                self.advance();
                let escaped = self.cur_char();
                match escaped {
                    '"' => s.push('"'),
                    '\\' => s.push('\\'),
                    'n' => s.push('\n'),
                    't' => s.push('\t'),
                    _ => s.push(escaped),
                }
                self.advance();
            } else {
                s.push(c);
                self.advance_char(c);
            }
        }
    }

    fn read_until_char(&mut self, stop: char) -> Result<String, Diagnostic> {
        let start = self.pos;
        while self.pos < self.src.len() && self.cur_char() != stop {
            let c = self.cur_char();
            self.advance_char(c);
        }
        if self.pos >= self.src.len() {
            return Err(self.e1206(self.line, &format!("expected `{}` not found", stop)));
        }
        Ok(self.src[start..self.pos].to_string())
    }

    fn expect_char(&mut self, expected: char, msg: &str) -> Result<(), Diagnostic> {
        if self.pos >= self.src.len() || self.cur_char() != expected {
            return Err(self.e1206(self.line, msg));
        }
        self.advance();
        Ok(())
    }

    fn skip_whitespace_and_comments(&mut self) {
        while self.pos < self.src.len() {
            let c = self.cur_char();
            if c == '#' {
                self.skip_to_eol();
            } else if c == '\n' {
                self.line += 1;
                self.pos += 1;
            } else if c.is_whitespace() {
                self.advance_char(c);
            } else {
                break;
            }
        }
    }

    fn skip_inline_ws(&mut self) {
        while self.pos < self.src.len() {
            let c = self.cur_char();
            if c == ' ' || c == '\t' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn skip_to_eol(&mut self) {
        while self.pos < self.src.len() {
            let c = self.cur_char();
            if c == '\n' {
                break;
            }
            self.advance_char(c);
        }
    }

    fn cur_char(&self) -> char {
        self.src[self.pos..].chars().next().unwrap_or('\0')
    }

    fn advance(&mut self) {
        let c = self.cur_char();
        self.advance_char(c);
    }

    fn advance_char(&mut self, c: char) {
        self.pos += c.len_utf8();
    }

    fn e1206(&self, line_no: usize, detail: &str) -> Diagnostic {
        e1206(self.file, line_no, detail)
    }
}

fn e1206(_file: &str, line_no: usize, detail: &str) -> Diagnostic {
    let what = if line_no > 0 {
        format!("`jet.toml` has a shape error on line {}", line_no)
    } else {
        "`jet.toml` has a shape error".to_string()
    };
    Diagnostic::error(
        "E1206",
        what,
        "jet.toml uses a small subset of TOML — tables, strings, and inline tables only"
            .to_string(),
        format!(
            "check `jet.toml` for unclosed quotes, missing `=`, or unsupported TOML features: {}",
            detail
        ),
        None,
    )
}

pub fn e1209(_file: &str, section: &str) -> Diagnostic {
    Diagnostic::error(
        "E1209",
        format!("`[{}]` is reserved and not yet implemented", section),
        "this section name is reserved for a future Jet feature — using it now is an error"
            .to_string(),
        format!(
            "remove the `[{}]` table from `jet.toml`, or leave it empty",
            section
        ),
        None,
    )
}
