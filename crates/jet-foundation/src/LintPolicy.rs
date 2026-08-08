//! D-LINTPOLICY1=A: one package lint wall for every sema lint.

use crate::Diagnostics::{Diagnostic, Severity};
use crate::Syntax;

/// Parse the `lints:` part of a package policy.
pub fn parse_policy_lints(body: &str) -> Result<Option<Vec<String>>, String> {
    let Some(lints_body) = block_body(body, Syntax::POLICY_FIELD_LINTS, '{', '}') else {
        return Ok(None);
    };
    let mut deny = Vec::new();
    for (key, value) in key_value_entries(&lints_body) {
        if key == Syntax::LINTS_FIELD_DENY {
            deny = parse_lint_code_list(value.trim())?;
        } else {
            return Err(format!(
                "unknown `policy.lints` field `{key}` — allowed: `{}`",
                Syntax::LINTS_FIELD_DENY,
            ));
        }
    }
    Ok(Some(deny))
}

/// Parse package source with the same `policy.lints.deny` reader used by the
/// Package model. Loader validation owns malformed-package diagnostics; this
/// read only supplies the already-validated policy to sema.
pub fn parse_package_source(source: &str) -> Result<Option<Vec<String>>, String> {
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
    deny.iter().any(|denied| denied == code)
}

fn e1293(original: &Diagnostic) -> Diagnostic {
    let why = original.why.trim_end_matches('.');
    Diagnostic::error(
        "E1293",
        format!(
            "lint `{}` is denied by policy: {}",
            original.code, original.what
        ),
        format!(
            "{why}. This team's `policy.lints.deny` in `pkg.jet` turns this warning into a build failure (D-LINTPOLICY1 — the override law); it stays a warning everywhere `pkg.jet` doesn't opt in."
        ),
        original.fix.clone(),
        original.span,
    )
}

fn parse_lint_code_list(value: &str) -> Result<Vec<String>, String> {
    let inner = value
        .trim()
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .ok_or_else(|| {
            format!(
                "`{}:` must be a list like `[L0504, L2401]`",
                Syntax::LINTS_FIELD_DENY
            )
        })?;
    let mut names = Vec::new();
    for entry in top_level_commas(inner) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let name = unquote(entry);
        if !is_lint_code_shape(&name) {
            return Err(format!(
                "`{name}` isn't shaped like a lint code (`L` + 4 digits)"
            ));
        }
        names.push(name);
    }
    Ok(names)
}

fn is_lint_code_shape(value: &str) -> bool {
    let mut chars = value.chars();
    if chars.next() != Some('L') {
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
        let codes = parse_package_source(
            r#"policy: .{ lints: .{ deny: [L0302, "L0504"] } }"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(codes, vec!["L0302".to_string(), "L0504".to_string()]);
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
        let out = apply(&["L0302".to_string()], vec![lint]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].code, "E1293");
        assert_eq!(out[0].severity, Severity::Error);
    }
}
