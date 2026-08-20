//! User-facing syntax dictionary.
//!
//! Rows start with `JET_HIGHLIGHT_TOKENS`, which already references the
//! canonical `Syntax` constants. Decision IDs come from those constants'
//! source comments or the applied-rule registry; no second token list exists.

use std::sync::LazyLock;

use super::{highlighted_tokens_sorted, HighlightClass, HighlightToken, JET_KEYWORD_LIST};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyntaxDictionaryKind {
    Keyword,
    Marker,
    Operator,
    Sigil,
    Literal,
    Type,
    Builtin,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntaxDictionaryRow {
    pub token: String,
    pub name: String,
    pub meaning: String,
    pub decision: String,
    pub example: String,
    pub kind: SyntaxDictionaryKind,
}

static ROWS: LazyLock<Vec<SyntaxDictionaryRow>> = LazyLock::new(build_rows);

pub fn rows() -> &'static [SyntaxDictionaryRow] {
    &ROWS
}

pub fn lookup(query: &str) -> Option<&'static SyntaxDictionaryRow> {
    let query = query.trim();
    rows().iter().find(|row| {
        row.token.eq_ignore_ascii_case(query)
            || display(row).eq_ignore_ascii_case(query)
            || row.kind == SyntaxDictionaryKind::Marker
                && row.token.eq_ignore_ascii_case(query.strip_prefix('#').unwrap_or(query))
    })
}

pub fn nearest(query: &str) -> Option<&'static SyntaxDictionaryRow> {
    let query = query.trim();
    rows().iter().min_by_key(|row| {
        levenshtein(
            &query.to_ascii_lowercase(),
            &display(row).to_ascii_lowercase(),
        )
    })
}

pub fn looks_like_query(query: &str) -> bool {
    let query = query.trim();
    if query.starts_with("package-overlay:") {
        return false;
    }
    if is_package_ref(query) {
        return false;
    }
    query.starts_with('@')
        || query.starts_with('#')
        || query.starts_with("::")
        || query.starts_with(":=")
        || query.chars().any(|ch| {
            !ch.is_ascii_alphanumeric() && !matches!(ch, '_' | '.' | '-' | '/' | '@')
        })
}

fn is_package_ref(query: &str) -> bool {
    if query.contains('@') && !query.starts_with('@') {
        return true;
    }
    query.split_once(':').is_some_and(|(prefix, suffix)| {
        !prefix.is_empty()
            && !suffix.is_empty()
            && prefix.chars().all(|ch| {
                ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/')
            })
            && suffix.chars().any(|ch| ch.is_ascii_alphanumeric())
    })
}

fn build_rows() -> Vec<SyntaxDictionaryRow> {
    let decisions = source_decisions();
    let acronym_decision = source_header_decision(include_str!("acronyms.rs"));
    let mut rows = Vec::new();
    let mut tokens = highlighted_tokens_sorted();
    for text in [super::MARKER_REGION, super::MARKER_LIVE, super::MARKER_NONDETERMINISTIC] {
        if !tokens.iter().any(|token| token.text == text) {
            tokens.push(HighlightToken {
                text,
                class: HighlightClass::MarkerRule,
            });
        }
    }
    for &text in JET_KEYWORD_LIST {
        if !tokens.iter().any(|token| token.text == text) {
            tokens.push(HighlightToken {
                text,
                class: HighlightClass::KeywordOther,
            });
        }
    }
    for row in crate::Registry::rows()
        .iter()
        .filter(|row| row.kind() == crate::Registry::RowKind::Marker)
    {
        if !tokens.iter().any(|token| token.text == row.name) {
            tokens.push(HighlightToken {
                text: row.name,
                class: HighlightClass::MarkerRule,
            });
        }
    }
    for token in tokens {
        if rows.iter().any(|row: &SyntaxDictionaryRow| row.token == token.text) {
            continue;
        }
        let kind = kind(token.class);
        let (name, decision) = metadata(
            &decisions,
            token.text,
            kind,
            acronym_decision.as_deref(),
        );
        rows.push(SyntaxDictionaryRow {
            token: token.text.to_string(),
            name,
            meaning: meaning(token.text, kind),
            decision,
            example: example(token.text, kind),
            kind,
        });
    }
    rows
}

fn metadata(
    decisions: &[(String, (String, String))],
    token: &str,
    kind: SyntaxDictionaryKind,
    acronym_decision: Option<&str>,
) -> (String, String) {
    let mut candidates = decisions
        .iter()
        .filter(|(value, (_, decision))| value.as_str() == token && decision != "Syntax.rs");
    let candidate = candidates
        .find(|(_, (name, _))| constant_matches_kind(name, kind))
        .or_else(|| {
            decisions
                .iter()
                .find(|(value, (_, decision))| {
                    value.as_str() == token && decision != "Syntax.rs"
                })
        });
    if let Some((_, metadata)) = candidate {
        return metadata.clone();
    }
    if let Some(row) = crate::Registry::row(token) {
        return (token.to_string(), row.decision.to_string());
    }
    if is_acronym(token) {
        if let Some(decision) = acronym_decision {
            return (token.to_string(), decision.to_string());
        }
    }
    (token.to_string(), "Syntax.rs".to_string())
}

fn constant_matches_kind(name: &str, kind: SyntaxDictionaryKind) -> bool {
    match kind {
        SyntaxDictionaryKind::Keyword => {
            name.starts_with("KW_") || name.starts_with("LIT_") || name.starts_with("BUILTIN_")
        }
        SyntaxDictionaryKind::Marker => name.starts_with("MARKER_") || name.starts_with("RULE_"),
        SyntaxDictionaryKind::Operator => name.starts_with("OP_"),
        SyntaxDictionaryKind::Sigil => {
            name.starts_with("SIGIL_")
                || name.ends_with("_PREFIX")
                || name.ends_with("_MARK")
                || name == "TYPE_FIXED_SIZE_SEP"
        }
        SyntaxDictionaryKind::Literal => name.starts_with("LIT_"),
        SyntaxDictionaryKind::Type => name.starts_with("TYPE_") || name.ends_with("_TYPE"),
        SyntaxDictionaryKind::Builtin => name.starts_with("BUILTIN_"),
    }
}

fn source_header_decision(source: &str) -> Option<String> {
    source.lines().take(24).find_map(decision_id).map(str::to_string)
}

fn is_acronym(token: &str) -> bool {
    token.chars().filter(|ch| ch.is_ascii_uppercase()).count() >= 2
        && token.chars().any(|ch| ch.is_ascii_lowercase())
}

fn kind(class: HighlightClass) -> SyntaxDictionaryKind {
    match class {
        HighlightClass::KeywordControl
        | HighlightClass::KeywordDeclaration
        | HighlightClass::KeywordOwnership
        | HighlightClass::KeywordOther => SyntaxDictionaryKind::Keyword,
        HighlightClass::MarkerRule => SyntaxDictionaryKind::Marker,
        HighlightClass::Operator => SyntaxDictionaryKind::Operator,
        HighlightClass::Sigil => SyntaxDictionaryKind::Sigil,
        HighlightClass::Literal => SyntaxDictionaryKind::Literal,
        HighlightClass::TypeBuiltin => SyntaxDictionaryKind::Type,
        HighlightClass::Builtin => SyntaxDictionaryKind::Builtin,
    }
}

fn meaning(token: &str, kind: SyntaxDictionaryKind) -> String {
    match token {
        super::COMPTIME_MARK => "compile-time demand mark on a name or block".to_string(),
        super::SIGIL_BIND_IMMUT => "immutable binding or one-line function-body marker".to_string(),
        super::MARKER_LIVE => "terminal direct-input block marker".to_string(),
        super::OP_UNIFIED_ARROW => "callable, arm, and lambda arrow".to_string(),
        _ => match kind {
            SyntaxDictionaryKind::Keyword => format!("registered Jet keyword `{token}`"),
            SyntaxDictionaryKind::Marker => format!("registered marker `#{token}`"),
            SyntaxDictionaryKind::Operator => format!("registered operator `{token}`"),
            SyntaxDictionaryKind::Sigil => format!("registered sigil `{token}`"),
            SyntaxDictionaryKind::Literal => format!("registered literal `{token}`"),
            SyntaxDictionaryKind::Type => format!("registered built-in type `{token}`"),
            SyntaxDictionaryKind::Builtin => format!("registered built-in `{token}`"),
        },
    }
}

fn example(token: &str, kind: SyntaxDictionaryKind) -> String {
    match token {
        super::COMPTIME_MARK => "@limit :: 1000".to_string(),
        super::SIGIL_BIND_IMMUT => "answer :: 42".to_string(),
        super::MARKER_LIVE => "#Live { input() }".to_string(),
        super::OP_UNIFIED_ARROW => "fn twice(n: Int) :> Int :: n * 2".to_string(),
        _ => match kind {
            SyntaxDictionaryKind::Keyword => format!("{token} …"),
            SyntaxDictionaryKind::Marker => format!("#{token} …"),
            SyntaxDictionaryKind::Operator => format!("left {token} right"),
            SyntaxDictionaryKind::Sigil => format!("name {token} value"),
            SyntaxDictionaryKind::Literal
            | SyntaxDictionaryKind::Type
            | SyntaxDictionaryKind::Builtin => token.to_string(),
        },
    }
}

pub fn display(row: &SyntaxDictionaryRow) -> String {
    if row.kind == SyntaxDictionaryKind::Marker {
        format!("#{}", row.token)
    } else {
        row.token.clone()
    }
}

fn source_decisions() -> Vec<(String, (String, String))> {
    const SOURCES: &[&str] = &[
        include_str!("../Syntax.rs"),
        include_str!("core_surface.rs"),
        include_str!("effects_surface.rs"),
        include_str!("math_layout.rs"),
        include_str!("markers.rs"),
        include_str!("package_files.rs"),
        include_str!("jetpack_config.rs"),
        include_str!("predicates.rs"),
    ];
    let mut out = Vec::new();
    for source in SOURCES {
        let mut comments = String::new();
        let mut inherited_comments = String::new();
        for line in source.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") {
                comments.push_str(trimmed);
                comments.push('\n');
                continue;
            }
            let Some(rest) = trimmed.strip_prefix("pub const ") else {
                comments.clear();
                inherited_comments.clear();
                continue;
            };
            let Some((name, rhs)) = rest.split_once(':') else {
                comments.clear();
                continue;
            };
            let Some(value) = rhs.split_once("= \"").and_then(|(_, rest)| rest.split_once('"'))
            else {
                comments.clear();
                continue;
            };
            let decision = decision_id(trimmed)
                .or_else(|| comment_decision(&comments))
                .or_else(|| decision_id(&inherited_comments))
                .unwrap_or("Syntax.rs")
                .to_string();
            if !comments.is_empty() {
                inherited_comments.clone_from(&comments);
            }
            out.push((
                value.0.to_string(),
                (name.trim().to_string(), decision),
            ));
            comments.clear();
        }
    }
    out
}

fn decision_id(text: &str) -> Option<&str> {
    text.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-')
        .find(|word| {
            word.starts_with("D-")
                || word.starts_with('S') && word[1..].chars().all(|ch| ch.is_ascii_digit())
                || word.starts_with('M') && word[1..].chars().all(|ch| ch.is_ascii_digit())
                || word.starts_with('U') && word[1..].chars().all(|ch| ch.is_ascii_digit())
        })
}

fn comment_decision(text: &str) -> Option<&str> {
    text.lines().rev().find_map(decision_id)
}

fn levenshtein(left: &str, right: &str) -> usize {
    let mut row: Vec<usize> = (0..=right.chars().count()).collect();
    for (i, left) in left.chars().enumerate() {
        let mut next = vec![i + 1];
        for (j, right) in right.chars().enumerate() {
            next.push(if left == right {
                row[j]
            } else {
                1 + row[j].min(row[j + 1]).min(next[j])
            });
        }
        row = next;
    }
    row[right.chars().count()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictionary_covers_highlights_and_keywords_without_foreign_words() {
        for token in crate::Syntax::JET_HIGHLIGHT_TOKENS {
            assert!(lookup(token.text).is_some(), "missing highlight token `{}`", token.text);
        }
        for token in JET_KEYWORD_LIST {
            assert!(lookup(token).is_some(), "missing keyword `{token}`");
        }

        let constants = source_decisions();
        for row in rows() {
            assert!(
                constants.iter().any(|(token, _)| token == &row.token)
                    || crate::Registry::row(&row.token).is_some(),
                "dictionary token `{}` is not a Syntax constant or registry marker",
                row.token
            );
            assert_ne!(row.decision, "Syntax.rs", "missing decision ID for `{}`", row.token);
            assert!(!row.example.is_empty(), "missing example for `{}`", row.token);
        }
    }
}
