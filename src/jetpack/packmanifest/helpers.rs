//! Small structural string helpers for the `pkg.jet` manifest surface
//! (std-only, comment-stripped input).

// ── small structural helpers (std-only, comment-stripped input) ──────────────

/// Remove `//` line comments, preserving the rest of each line. (Block comments
/// and string-embedded `//` are out of scope for the manifest surface.)
pub(super) fn strip_line_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        out.push_str(&strip_line_comment(line));
        out.push('\n');
    }
    out
}

/// Find the `//` that starts a line comment, ignoring one embedded in a
/// quoted string (e.g. a git URL: `"https://github.com/acme/parsekit"`).
fn strip_line_comment(line: &str) -> &str {
    let mut in_string = false;
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => in_string = !in_string,
            b'/' if !in_string && bytes.get(i + 1) == Some(&b'/') => return &line[..i],
            _ => {}
        }
        i += 1;
    }
    line
}

/// The body inside the `open`/`close` delimiters following `key:` at the top
/// level, with balanced nesting. Returns `None` if `key:` (followed by `open`)
/// is absent.
pub(super) fn block_body(text: &str, key: &str, open: char, close: char) -> Option<String> {
    let mut search_from = 0;
    while let Some(rel) = text[search_from..].find(key) {
        let at = search_from + rel;
        // Require a word boundary before `key` so `deps` doesn't match inside a
        // longer identifier.
        let preceded_ok = at == 0
            || !text[..at]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
        let after = &text[at + key.len()..];
        let after_trim = after.trim_start();
        if preceded_ok && after_trim.starts_with(':') {
            let rest = after_trim[1..].trim_start();
            if let Some(stripped) = rest.strip_prefix(open) {
                return Some(balanced(stripped, open, close));
            }
        }
        search_from = at + key.len();
    }
    None
}

/// Capture text up to the matching `close`, honoring nested `open`/`close`.
fn balanced(s: &str, open: char, close: char) -> String {
    let mut depth = 1;
    let mut out = String::new();
    for c in s.chars() {
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                break;
            }
        }
        out.push(c);
    }
    out
}

/// Split a `{ … }` body into `key: value` entries. Splits entries on commas at
/// the top nesting level (so a value may itself contain brackets), then splits
/// each entry on its first `:`.
pub(super) fn key_value_entries(body: &str) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    for entry in top_level_commas(body) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        if let Some((k, v)) = entry.split_once(':') {
            entries.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    entries
}

/// Split on commas that are not nested inside `()`/`[]`/`{}`.
pub(super) fn top_level_commas(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for c in body.chars() {
        match c {
            '(' | '[' | '{' => {
                depth += 1;
                cur.push(c);
            }
            ')' | ']' | '}' => {
                depth -= 1;
                cur.push(c);
            }
            ',' if depth == 0 => {
                out.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// Strip surrounding double quotes if present; otherwise return as-is, trimmed.
pub(super) fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}
