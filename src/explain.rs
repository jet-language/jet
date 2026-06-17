//! `jet explain <CODE>` — offline terminal essays for every diagnostic code.
//!
//! The index is built from the spec itself: `docs/spec/diagnostics.md` is
//! embedded at compile time (`include_str!`), so `explain` works with no
//! network and no files on disk. Every code in the registry table gets an
//! entry by construction (invariant I4: no code without an explain), and any
//! code that also has a detailed *what/why/fix* block gets the richer essay.

use std::collections::BTreeMap;

/// The embedded diagnostics spec — the single source of truth for codes.
const DIAGNOSTICS_MD: &str = include_str!("../docs/spec/diagnostics.md");

/// One explainable diagnostic code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Explanation {
    pub code: String,
    /// Pipeline stage (`jet` / `parse` / `sema` / …), from the registry table.
    pub stage: String,
    /// One-line meaning from the registry table (always present).
    pub meaning: String,
    /// Plain-language "what happened", from a detailed table (when present).
    pub what: Option<String>,
    /// The rule behind it, from a detailed table (when present).
    pub why: Option<String>,
    /// A concrete next step, from a detailed table (when present).
    pub fix: Option<String>,
    /// True when the registry marks the code as retired (kept for history).
    pub retired: bool,
}

/// Build the full code → explanation index from the embedded spec.
pub fn index() -> BTreeMap<String, Explanation> {
    let mut out: BTreeMap<String, Explanation> = BTreeMap::new();

    // Pass 1: the registry table (`| Code | Stage | Meaning |`) gives every
    // code its stage + one-line meaning.
    for row in table_rows(DIAGNOSTICS_MD) {
        if row.len() != 3 {
            continue;
        }
        let code = row[0].trim();
        if !is_code(code) {
            continue;
        }
        let meaning = row[2].trim().to_string();
        let retired = meaning.contains("retired");
        out.insert(
            code.to_string(),
            Explanation {
                code: code.to_string(),
                stage: row[1].trim().to_string(),
                meaning,
                what: None,
                why: None,
                fix: None,
                retired,
            },
        );
    }

    // Pass 2: detailed tables (`| Code | What | Why | Fix |`) enrich entries.
    for row in table_rows(DIAGNOSTICS_MD) {
        if row.len() != 4 {
            continue;
        }
        let code = row[0].trim();
        if !is_code(code) {
            continue;
        }
        let what = row[1].trim().to_string();
        let why = row[2].trim().to_string();
        let fix = row[3].trim().to_string();
        let entry = out.entry(code.to_string()).or_insert_with(|| Explanation {
            code: code.to_string(),
            stage: String::new(),
            meaning: what.clone(),
            what: None,
            why: None,
            fix: None,
            retired: false,
        });
        entry.what = Some(what);
        entry.why = Some(why);
        entry.fix = Some(fix);
    }

    out
}

/// Live codes (non-retired). Used to verify I4: every registered code
/// resolves through `jet explain`.
pub fn live_codes() -> Vec<String> {
    index()
        .into_values()
        .filter(|e| !e.retired)
        .map(|e| e.code)
        .collect()
}

/// Look up one code (case-insensitive).
pub fn lookup(code: &str) -> Option<Explanation> {
    let want = normalize(code);
    index().into_iter().find_map(|(k, v)| {
        if normalize(&k) == want {
            Some(v)
        } else {
            None
        }
    })
}

/// Render the offline essay for a code. Uses readable, beginner-friendly
/// headers. `color` bolds the code line on a TTY.
pub fn render(ex: &Explanation, color: bool) -> String {
    let mut out = String::new();
    let bold = |s: &str| {
        if color { format!("\x1b[1m{}\x1b[0m", s) } else { s.to_string() }
    };
    out.push_str(&format!("{}\n\n", bold(&ex.code)));
    if ex.retired {
        out.push_str("This code is retired: it is no longer produced by the current\n");
        out.push_str("compiler, and is kept here only so old build logs stay readable.\n");
        return out;
    }
    let what = ex.what.as_deref().or(Some(ex.meaning.as_str()));
    if let Some(w) = what {
        out.push_str(&format!("What this means:\n  {}\n\n", w));
    }
    if let Some(why) = &ex.why {
        out.push_str(&format!(
            "Why {} enforces it:\n  {}\n\n",
            crate::syntax::LANG_NAME,
            why
        ));
    }
    if let Some(fix) = &ex.fix {
        out.push_str(&format!("How to fix it:\n  {}\n\n", fix));
    }
    if ex.what.is_none() {
        // No detailed block yet: show stage so the entry is still useful.
        if !ex.stage.is_empty() {
            out.push_str(&format!("Stage: {}\n\n", ex.stage));
        }
        out.push_str(
            "A longer explanation will land with the detailed entry in docs/spec/diagnostics.md.\n\n",
        );
    }
    out.push_str(&format!(
        "This explanation comes from {}'s diagnostics reference.\n",
        crate::syntax::BINARY_NAME
    ));
    out
}

/// The teaching pointer appended after a rendered error (one dim line).
/// Suppressed in `--json` (the code is already structured there).
pub fn pointer_line(code: &str, color: bool) -> String {
    let body = format!("run `{} explain {}` to learn more", crate::syntax::BINARY_NAME, code);
    if color {
        format!("\x1b[2m{}\x1b[0m", body)
    } else {
        body
    }
}

fn normalize(code: &str) -> String {
    code.trim().to_uppercase()
}

fn is_code(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 5
        && (b[0] == b'E' || b[0] == b'L')
        && b[1..].iter().all(|c| c.is_ascii_digit())
}

/// Yield each markdown table row as a vector of trimmed cells. Separator rows
/// (`|---|---|`) and code-fenced blocks are skipped.
fn table_rows(md: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut in_fence = false;
    for line in md.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if !trimmed.starts_with('|') {
            continue;
        }
        let stripped: String = trimmed
            .chars()
            .filter(|c| !matches!(c, '|' | '-' | ':' | ' '))
            .collect();
        if stripped.is_empty() {
            continue;
        }
        let inner = trimmed.trim_matches('|');
        let cells: Vec<String> = split_cells(inner);
        rows.push(cells);
    }
    rows
}

/// Split a table row on `|`, honoring `\|` escapes inside backtick spans.
fn split_cells(s: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(&next) = chars.peek() {
                if next == '|' {
                    cur.push('|');
                    chars.next();
                    continue;
                }
            }
            cur.push('\\');
        } else if c == '|' {
            cells.push(cur.trim().to_string());
            cur = String::new();
        } else {
            cur.push(c);
        }
    }
    cells.push(cur.trim().to_string());
    cells
}
