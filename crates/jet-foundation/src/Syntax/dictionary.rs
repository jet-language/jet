//! User-facing syntax dictionary.
//!
//! Rows start with `JET_HIGHLIGHT_TOKENS`, which already references the
//! canonical `Syntax` constants. Decision IDs come from those constants'
//! source comments or the applied-rule registry; no second token list exists.

use std::sync::LazyLock;

use super::{highlighted_tokens_sorted, HighlightClass, HighlightToken};

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
    query.starts_with('@')
        || query.starts_with('#')
        || query.starts_with("::")
        || query.starts_with(":=")
        || (!query.is_empty()
            && query
                .chars()
                .all(|ch| !ch.is_ascii_alphanumeric() && ch != '_' && ch != '.'))
}

fn build_rows() -> Vec<SyntaxDictionaryRow> {
    let decisions = source_decisions();
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
        let (name, decision) = decisions
            .iter()
            .find(|(value, _)| value.as_str() == token.text)
            .map(|(_, metadata)| metadata.clone())
            .or_else(|| {
                crate::Registry::row(token.text)
                    .map(|row| (token.text.to_string(), row.decision.to_string()))
            })
            .unwrap_or_else(|| (token.text.to_string(), "Syntax.rs".to_string()));
        let kind = kind(token.class);
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
