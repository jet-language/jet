//! D-LINTPOLICY1=A: one package lint wall for every sema lint.

use crate::Diagnostics::{Diagnostic, Severity};
use crate::Syntax;

/// One stable user-facing name for one registered lint.
///
/// The diagnostic row remains the authority for the rendered code and prose;
/// this is the lint-selection registry used by every config surface. Keeping
/// the two fields together makes a policy value name-first while preserving
/// the code for rendered reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegisteredLint {
    pub name: &'static str,
    pub code: &'static str,
}

/// The package-level auto-derive refusal is a named entry in the shared lint
/// deny list. It is reserved because no source lint is emitted for the policy
/// itself; the loader consumes the same registry identity.
pub const AUTO_DERIVE_LINT: RegisteredLint = RegisteredLint {
    name: "auto_derive",
    code: "L0509",
};

/// Every registered lint has one stable snake_case name. Retired and reserved
/// rows stay registered so old reports and future migrations keep one identity.
pub const REGISTERED_LINTS: &[RegisteredLint] = &[
    RegisteredLint { name: "dead_end_state", code: "L0151" },
    RegisteredLint { name: "divergent_state_paths", code: "L0152" },
    RegisteredLint { name: "implicit_clone", code: "L0201" },
    RegisteredLint { name: "shared_auto_clone", code: "L0202" },
    RegisteredLint { name: "unpinned_script_dependency", code: "L0203" },
    RegisteredLint { name: "untranslated_flake_field", code: "L0204" },
    RegisteredLint { name: "unsandboxed_build_fallback", code: "L0205" },
    RegisteredLint { name: "shared_guard_long_scope", code: "L0206" },
    RegisteredLint { name: "compiler_extension", code: "L1401" },
    RegisteredLint { name: "unreachable_dispatch_arm", code: "L0301" },
    RegisteredLint { name: "same_enum_guard_table", code: "L0302" },
    RegisteredLint { name: "slice_copy_in_loop", code: "L0501" },
    RegisteredLint { name: "float_comparison", code: "L0502" },
    RegisteredLint { name: "compound_assignment", code: "L0503" },
    RegisteredLint { name: "float_money", code: "L0504" },
    RegisteredLint { name: "heap_growth_in_loop", code: "L0505" },
    RegisteredLint { name: "hidden_context_allocation", code: "L0506" },
    RegisteredLint { name: "branch_arm_table", code: "L0507" },
    AUTO_DERIVE_LINT,
    RegisteredLint { name: "prelude_alias_shadow", code: "L0510" },
    RegisteredLint { name: "err_fallback_shadow", code: "L0511" },
    RegisteredLint { name: "display_migration", code: "L0520" },
    RegisteredLint { name: "soft_public_use", code: "L0601" },
    RegisteredLint { name: "inline_autodiff", code: "L1141" },
    RegisteredLint { name: "bare_unsafe", code: "L3101" },
    RegisteredLint { name: "impure_reason", code: "L3102" },
    RegisteredLint { name: "unjoined_task", code: "L1101" },
    RegisteredLint { name: "random_crypto_context", code: "W0410" },
    RegisteredLint { name: "raw_reference_view", code: "L2301" },
    RegisteredLint { name: "deprecated_item", code: "L2001" },
    RegisteredLint { name: "doctor_advisory", code: "L2101" },
    RegisteredLint { name: "regex_backtracking", code: "L2701" },
    RegisteredLint { name: "missing_accessible_label", code: "E2930" },
    RegisteredLint { name: "duplicate_accessible_label", code: "E2931" },
    RegisteredLint { name: "duplicate_layout_constraint", code: "E2934" },
    RegisteredLint { name: "positional_bool_parameter", code: "L2401" },
    RegisteredLint { name: "whole_file_read", code: "L2501" },
    RegisteredLint { name: "blocking_accept_loop", code: "L2801" },
    RegisteredLint { name: "empty_test", code: "L2901" },
];

/// The complete lint-selection registry.
pub const fn registered_lints() -> &'static [RegisteredLint] {
    REGISTERED_LINTS
}

/// Resolve a lint name to its rendered diagnostic code.
pub fn code_for_name(name: &str) -> Option<&'static str> {
    REGISTERED_LINTS
        .iter()
        .find_map(|lint| (lint.name == name).then_some(lint.code))
}

/// Resolve a rendered diagnostic code to its stable lint name.
pub fn name_for_code(code: &str) -> Option<&'static str> {
    REGISTERED_LINTS
        .iter()
        .find_map(|lint| (lint.code == code).then_some(lint.name))
}

/// Build the registered manifest fix for a lint-policy value that used a
/// diagnostic code. The manifest loader and the driver share this wording.
pub fn policy_error_fix(detail: &str) -> String {
    detail
        .split_once("use `")
        .and_then(|(_, rest)| rest.split_once('`'))
        .map(|(name, _)| {
            format!("use `{name}` in `policy.lints.deny` instead of the diagnostic code")
        })
        .unwrap_or_else(|| {
            "use a registered lint name in `policy.lints.deny` instead of a diagnostic code"
                .to_string()
        })
}

/// Parse the `lints:` part of a package policy.
pub fn parse_policy_lints(body: &str) -> Result<Option<Vec<String>>, String> {
    let Some(lints_body) = block_body(body, Syntax::POLICY_FIELD_LINTS, '{', '}') else {
        return Ok(None);
    };
    let mut deny = Vec::new();
    for (key, value) in key_value_entries(&lints_body) {
        if key == Syntax::LINTS_FIELD_DENY {
            deny = parse_lint_name_list(value.trim())?;
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

fn parse_lint_name_list(value: &str) -> Result<Vec<String>, String> {
    let inner = value
        .trim()
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .ok_or_else(|| {
            format!(
                "`{}:` must be a list like `[same_enum_guard_table, float_money]`",
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
        let Some(_) = code_for_name(&name) else {
            if let Some(canonical_name) = name_for_code(&name) {
                return Err(format!(
                    "`{name}` is a diagnostic code; use `{canonical_name}` in `policy.lints.deny`"
                ));
            }
            if is_diagnostic_code_shape(&name) {
                return Err(format!(
                    "`{name}` is a diagnostic code; `policy.lints.deny` takes named lint values"
                ));
            }
            return Err(format!(
                "`{name}` isn't a registered lint policy name; allowed: {}",
                known_lint_policy_names()
            ));
        };
        names.push(name);
    }
    Ok(names)
}

fn known_lint_policy_names() -> String {
    REGISTERED_LINTS
        .iter()
        .map(|lint| format!("`{}`", lint.name))
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
        assert!(error.contains("use `same_enum_guard_table`"));
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
}
