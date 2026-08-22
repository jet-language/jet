//! D-LINTPOLICY1=A: one package lint wall for every sema lint.

use crate::Diagnostics::{Diagnostic, Severity, Span};
use crate::Registry::DiagnosticRow;
use crate::Syntax;
use std::fmt;

/// One parse failure from a lint-selection surface. Code/name are present only
/// when the rejected value is a registered diagnostic code, so the caller can
/// fill the typed diagnostic row without parsing user-facing prose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintPolicyError {
    pub detail: String,
    pub code: Option<String>,
    pub name: Option<String>,
}

impl LintPolicyError {
    fn message(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
            code: None,
            name: None,
        }
    }

    fn diagnostic_code(detail: impl Into<String>, code: &str, name: Option<&str>) -> Self {
        Self {
            detail: detail.into(),
            code: Some(code.to_string()),
            name: name.map(str::to_string),
        }
    }
}

impl fmt::Display for LintPolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.detail)
    }
}

/// The complete lint-selection registry, projected from the typed diagnostic
/// rows. No config surface owns a second code/name table.
pub fn registered_lints() -> impl Iterator<Item = &'static DiagnosticRow> {
    crate::Registry::lint_rows()
}

/// Resolve the reserved auto-derive policy entry from the same lint registry.
pub fn auto_derive_lint() -> &'static DiagnosticRow {
    lint_by_name("auto_derive").expect("auto_derive must stay registered")
}

pub fn lint_by_name(name: &str) -> Option<&'static DiagnosticRow> {
    registered_lints().find(|lint| lint.lint_name == Some(name))
}

/// Resolve a lint name to its rendered diagnostic code.
pub fn code_for_name(name: &str) -> Option<&'static str> {
    lint_by_name(name).map(|lint| lint.code)
}

/// Resolve a rendered diagnostic code to its stable lint name.
pub fn name_for_code(code: &str) -> Option<&'static str> {
    crate::Registry::diagnostic(code).and_then(|lint| lint.lint_name)
}

/// Build the registered refusal for a source-level lint allowance that used
/// a rendered diagnostic code. `#allow(...)` is a user-typed lint selector,
/// so it follows the same name-only law as package policy.
pub fn selection_code_error(value: &str, surface: &str, span: Option<Span>) -> Option<Diagnostic> {
    if !is_diagnostic_code_shape(value) {
        return None;
    }
    let (what, fix) = if let Some(name) = name_for_code(value) {
        (
            format!("`{value}` is a diagnostic code; {surface} takes lint names"),
            format!("write `{name}` in {surface} instead of `{value}`"),
        )
    } else {
        (
            format!("`{value}` is a diagnostic code; {surface} takes registered lint names"),
            format!("write a registered lint name in {surface} instead of `{value}`"),
        )
    };
    Some(Diagnostic::error(
        "E0927",
        what,
        "lint selectors use stable snake_case names; diagnostic codes are rendered report identities"
            .to_string(),
        fix,
        span,
    ))
}

/// Parse the `lints:` part of a package policy.
pub fn parse_policy_lints(body: &str) -> Result<Option<Vec<String>>, LintPolicyError> {
    let Some(lints_body) = block_body(body, Syntax::POLICY_FIELD_LINTS, '{', '}') else {
        return Ok(None);
    };
    let mut deny = Vec::new();
    for (key, value) in key_value_entries(&lints_body) {
        if key == Syntax::LINTS_FIELD_DENY {
            deny = parse_lint_name_list(value.trim())?;
        } else {
            return Err(LintPolicyError::message(format!(
                "unknown `policy.lints` field `{key}` — allowed: `{}`",
                Syntax::LINTS_FIELD_DENY,
            )));
        }
    }
    Ok(Some(deny))
}

/// Parse package source with the same `policy.lints.deny` reader used by the
/// Package model. Loader validation owns malformed-package diagnostics; this
/// read only supplies the already-validated policy to sema.
pub fn parse_package_source(source: &str) -> Result<Option<Vec<String>>, LintPolicyError> {
    let source = strip_comments(source);
    let Some(policy_body) = block_body(&source, Syntax::MANIFEST_BLOCK_POLICY, '{', '}') else {
        return Ok(None);
    };
    parse_policy_lints(&policy_body)
}

/// Apply the package wall to every lint emitted by one sema run.
pub fn apply(deny: &[String], diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
    diagnostics
        .into_iter()
        .map(|diagnostic| {
            if diagnostic.severity == Severity::Lint && is_denied(deny, &diagnostic.code) {
                e1293(&diagnostic)
            } else {
                diagnostic
            }
        })
        .collect()
}

pub fn is_denied(deny: &[String], code: &str) -> bool {
    let Some(name) = name_for_code(code) else {
        return false;
    };
    deny.iter().any(|denied| denied == name)
}

fn e1293(original: &Diagnostic) -> Diagnostic {
    let name = name_for_code(&original.code).unwrap_or(&original.code);
    let why = original.why.trim_end_matches('.');
    Diagnostic::from_row(
        "E1293",
        &[
            ("name", name),
            ("code", original.code.as_str()),
            ("what", original.what.as_str()),
            ("why", why),
            ("fix", original.fix.as_str()),
        ],
        original.span,
    )
}

fn parse_lint_name_list(value: &str) -> Result<Vec<String>, LintPolicyError> {
    let inner = value
        .trim()
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .ok_or_else(|| {
            LintPolicyError::message(format!(
                "`{}:` must be a list like `[same_enum_guard_table, float_money]`",
                Syntax::LINTS_FIELD_DENY
            ))
        })?;
    let mut names = Vec::new();
    for entry in top_level_commas(inner) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let name = unquote(entry);
        let Some(_) = code_for_name(&name) else {
            if let Some(canonical_name) = name_for_code(&name) {
                return Err(LintPolicyError::diagnostic_code(
                    format!(
                        "`{name}` is a diagnostic code; use `{canonical_name}` in `policy.lints.deny`"
                    ),
                    &name,
                    Some(canonical_name),
                ));
            }
            if is_diagnostic_code_shape(&name) {
                return Err(LintPolicyError::diagnostic_code(
                    format!(
                        "`{name}` is a diagnostic code; `policy.lints.deny` takes named lint values"
                    ),
                    &name,
                    None,
                ));
            }
            return Err(LintPolicyError::message(format!(
                "`{name}` isn't a registered lint policy name; allowed: {}",
                known_lint_policy_names()
            )));
        };
        names.push(name);
    }
    Ok(names)
}

fn known_lint_policy_names() -> String {
    registered_lints()
        .map(|lint| format!("`{}`", lint.lint_name.expect("lint rows have names")))
        .collect::<Vec<_>>()
        .join(", ")
}

fn is_diagnostic_code_shape(value: &str) -> bool {
    let mut chars = value.chars();
    if !matches!(chars.next(), Some('E' | 'L' | 'W')) {
        return false;
    }
    let rest: Vec<char> = chars.collect();
    rest.len() == 4 && rest.iter().all(|c| c.is_ascii_digit())
}

fn block_body(text: &str, key: &str, open: char, close: char) -> Option<String> {
    let mut at = 0;
    let mut quoted = false;
    let mut escaped = false;
    while at < text.len() {
        let ch = text[at..].chars().next().expect("index is inside text");
        if quoted {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
            at += ch.len_utf8();
            continue;
        }
        if ch == '"' {
            quoted = true;
            at += ch.len_utf8();
            continue;
        }
        if !text[at..].starts_with(key) {
            at += ch.len_utf8();
            continue;
        }
        let preceded_ok = at == 0
            || !text[..at]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
        let after = &text[at + key.len()..];
        let after_trim = after.trim_start();
        if preceded_ok && after_trim.starts_with(':') {
            let rest = after_trim[1..].trim_start();
            let rest = rest.strip_prefix('.').unwrap_or(rest).trim_start();
            if let Some(stripped) = rest.strip_prefix(open) {
                return Some(balanced(stripped, open, close));
            }
        }
        at += ch.len_utf8();
    }
    None
}

fn balanced(source: &str, open: char, close: char) -> String {
    let mut depth = 1;
    let mut quoted = false;
    let mut escaped = false;
    let mut out = String::new();
    for c in source.chars() {
        if quoted {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                quoted = false;
            }
            continue;
        }
        if c == '"' {
            quoted = true;
            out.push(c);
        } else if c == open {
            depth += 1;
            out.push(c);
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                break;
            }
            out.push(c);
        } else {
            out.push(c);
        }
    }
    out
}

fn key_value_entries(body: &str) -> Vec<(String, String)> {
    top_level_commas(body)
        .into_iter()
        .filter_map(|entry| {
            let (key, value) = entry.trim().split_once(':')?;
            Some((key.trim().to_string(), value.trim().to_string()))
        })
        .collect()
}

fn top_level_commas(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for c in body.chars() {
        if quoted {
            current.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                quoted = false;
            }
            continue;
        }
        match c {
            '"' => {
                quoted = true;
                current.push(c);
            }
            '(' | '[' | '{' => {
                depth += 1;
                current.push(c);
            }
            ')' | ']' | '}' => {
                depth -= 1;
                current.push(c);
            }
            ',' if depth == 0 => out.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    if !current.trim().is_empty() {
        out.push(current);
    }
    out
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        value[1..value.len() - 1].to_string()
    } else {
        value.to_string()
    }
}

fn strip_comments(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut quoted = false;
    let mut escaped = false;
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if quoted {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
            continue;
        }
        if ch == '"' {
            quoted = true;
            out.push(ch);
        } else if ch == '/' && chars.peek() == Some(&'/') {
            chars.next();
            for comment_ch in chars.by_ref() {
                if comment_ch == '\n' {
                    out.push('\n');
                    break;
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_current_deny_spelling() {
        let names = parse_package_source(
            r#"policy: .{ lints: .{ deny: [same_enum_guard_table, "float_money", compiler_extension] } }"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            names,
            vec![
                "same_enum_guard_table".to_string(),
                "float_money".to_string(),
                "compiler_extension".to_string(),
            ]
        );
    }

    #[test]
    fn rejects_diagnostic_code_policy_value() {
        let code = ["L", "0302"].concat();
        let source = format!("policy: .{{ lints: .{{ deny: [{code}] }} }}");
        let error = parse_package_source(&source).unwrap_err();
        assert!(error.detail.contains("use `same_enum_guard_table`"));
    }

    #[test]
    fn promotes_denied_lint_once() {
        let lint = Diagnostic::lint(
            "L0302",
            "subjectless dispatch".to_string(),
            "name the subject".to_string(),
            "write a subject dispatch".to_string(),
            None,
        );
        let out = apply(&["same_enum_guard_table".to_string()], vec![lint]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].code, "E1293");
        assert_eq!(out[0].severity, Severity::Error);
        assert!(out[0].what.contains("same_enum_guard_table"));
    }

    #[test]
    fn promotes_unused_binding_lint_by_registered_name() {
        assert_eq!(code_for_name("unused_local_binding"), Some("L0101"));
        let lint = Diagnostic::from_row("L0101", &[("name", "value")], None);
        let out = apply(&["unused_local_binding".to_string()], vec![lint]);
        assert_eq!(out[0].code, "E1293");
        assert_eq!(out[0].severity, Severity::Error);
        assert!(out[0].what.contains("unused_local_binding"));
    }

    #[test]
    fn promotes_nested_subject_shorthand_lint_by_registered_name() {
        assert_eq!(code_for_name("subject_shorthand_nesting"), Some("L0512"));
        let deny = parse_package_source(
            "policy: .{ lints: .{ deny: [subject_shorthand_nesting] } }",
        )
        .unwrap()
        .unwrap();
        let lint = Diagnostic::lint(
            "L0512",
            "nested subject shorthand needs an explicit binding".to_string(),
            "implicit subjects become hard to track when shorthand scopes nest".to_string(),
            "rewrite the inner shorthand with a named binding".to_string(),
            None,
        );
        let out = apply(&deny, vec![lint]);
        assert_eq!(out[0].code, "E1293");
        assert_eq!(out[0].severity, Severity::Error);
        assert!(out[0].what.contains("subject_shorthand_nesting"));
    }
}
