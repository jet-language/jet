//! `jet explain <code>` — offline terminal essays for diagnostic codes.
//!
//! The rustc `--explain` idea, but in the terminal and offline. The single
//! source of truth is `docs/spec/diagnostics.md`: it is embedded in the binary
//! with `include_str!`, parsed into a `code -> entry` index, and rendered as a
//! short essay (what it means / why Jet enforces it / how to fix it). Deriving
//! from the canonical doc means the index can never silently drift from the
//! registry — a code added there is automatically explainable, and the
//! coverage test (tests/cli.rs) fails if any registered code can't round-trip.
//!
//! We do NOT invent policy here. Every line `jet explain` prints comes from
//! diagnostics.md (the registry "Meaning" column, or a detailed what/why/fix
//! table when the code has one).

/// The canonical diagnostics spec, embedded so `jet explain` works offline and
/// the index ships with the binary (cannot drift from the doc).
const DIAGNOSTICS_MD: &str = include_str!("../docs/spec/diagnostics.md");

/// One diagnostic's explain content, sourced from diagnostics.md.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub code: String,
    pub stage: Option<String>,
    /// One-line meaning from the registry table (always present).
    pub meaning: Option<String>,
    /// Full what/why/fix when the code appears in a detailed table.
    pub what: Option<String>,
    pub why: Option<String>,
    pub fix: Option<String>,
}

impl Entry {
    /// True when this code is a real, explainable diagnostic. *Retired* registry
    /// rows (e.g. E0004) are recorded but not surfaced as live codes.
    fn is_live(&self) -> bool {
        !self.is_retired()
    }

    fn is_retired(&self) -> bool {
        self.meaning
            .as_deref()
            .map(|m| m.contains("*retired") || m.starts_with("*retired"))
            .unwrap_or(false)
    }

    /// Render the offline essay in the diagnostics.md what/why/fix voice.
    pub fn essay(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("{}\n", self.code));
        out.push('\n');
        // Prefer the detailed what/why/fix when the code has one; otherwise the
        // registry's one-line meaning carries the "what".
        let what = self.what.clone().or_else(|| self.meaning.clone());
        if let Some(w) = &what {
            out.push_str(&format!("What this means:\n  {}\n", w));
            out.push('\n');
        }
        if let Some(why) = &self.why {
            out.push_str(&format!("Why {} enforces it:\n  {}\n", crate::syntax::LANG_NAME, why));
            out.push('\n');
        }
        if let Some(fix) = &self.fix {
            out.push_str(&format!("How to fix it:\n  {}\n", fix));
            out.push('\n');
        }
        if let Some(stage) = &self.stage {
            out.push_str(&format!("Stage: {}\n", stage));
        }
        out.push_str(&format!(
            "\nThis explanation comes from {}'s diagnostics reference.\n",
            crate::syntax::BINARY_NAME
        ));
        out
    }
}

/// Build the full `code -> Entry` index from the embedded diagnostics doc.
/// Detailed what/why/fix tables override/augment the registry one-liners.
pub fn index() -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();

    for row in markdown_table_rows(DIAGNOSTICS_MD) {
        // Registry table: | Code | Stage | Meaning |
        if row.len() == 3 && is_code(&row[0]) {
            upsert(&mut entries, &row[0], |e| {
                if e.stage.is_none() {
                    e.stage = Some(row[1].clone());
                }
                if e.meaning.is_none() {
                    e.meaning = Some(row[2].clone());
                }
            });
        }
        // Detailed table: | Code | What | Why | Fix |
        else if row.len() == 4 && is_code(&row[0]) {
            upsert(&mut entries, &row[0], |e| {
                e.what = Some(row[1].clone());
                e.why = Some(row[2].clone());
                e.fix = Some(row[3].clone());
            });
        }
    }
    entries
}

/// Live, explainable codes (retired rows excluded).
pub fn live_codes() -> Vec<String> {
    index()
        .into_iter()
        .filter(|e| e.is_live())
        .map(|e| e.code)
        .collect()
}

/// Look up a single code (case-insensitive). Returns None for unknown or
/// retired codes — both are surfaced as "no explanation" to the user.
pub fn lookup(code: &str) -> Option<Entry> {
    let want = code.trim().to_ascii_uppercase();
    index()
        .into_iter()
        .find(|e| e.code.eq_ignore_ascii_case(&want) && e.is_live())
}

fn upsert(entries: &mut Vec<Entry>, code: &str, f: impl FnOnce(&mut Entry)) {
    if let Some(e) = entries.iter_mut().find(|e| e.code == code) {
        f(e);
        return;
    }
    let mut e = Entry {
        code: code.to_string(),
        stage: None,
        meaning: None,
        what: None,
        why: None,
        fix: None,
    };
    f(&mut e);
    entries.push(e);
}

/// A code is `E####` or `L####` (the only registry shapes diagnostics.md uses).
fn is_code(cell: &str) -> bool {
    let c = cell.trim();
    let mut chars = c.chars();
    matches!(chars.next(), Some('E') | Some('L'))
        && c.len() >= 5
        && c[1..].chars().all(|ch| ch.is_ascii_digit())
}

/// Yield every markdown pipe-table data row in `md` as a vector of trimmed
/// cells. Header rows (`Code`/`Stage`/…) and separator rows (`---`) are skipped.
/// Backtick code-fenced regions are skipped so example output never parses as a
/// table.
fn markdown_table_rows(md: &str) -> Vec<Vec<String>> {
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
        // A markdown table row: split on '|', drop the empty leading/trailing.
        let cells: Vec<String> = trimmed
            .trim_matches('|')
            .split('|')
            .map(|c| c.trim().to_string())
            .collect();
        // Separator row, e.g. |---|---|.
        if cells.iter().all(|c| c.chars().all(|ch| ch == '-' || ch == ':') && !c.is_empty()) {
            continue;
        }
        rows.push(cells);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_codes_are_indexed() {
        let codes = live_codes();
        assert!(codes.contains(&"E2101".to_string()));
        assert!(codes.contains(&"E0102".to_string()));
        // A retired row is parsed but not live.
        assert!(!codes.contains(&"E0004".to_string()));
    }

    #[test]
    fn lookup_is_case_insensitive() {
        assert!(lookup("e2101").is_some());
        assert!(lookup("E2101").is_some());
        assert!(lookup("BOGUS").is_none());
    }

    #[test]
    fn detailed_table_overrides_registry() {
        // E2101 has a detailed what/why/fix row in the CLI diagnostics table.
        let e = lookup("E2101").unwrap();
        assert!(e.what.is_some());
        assert!(e.why.is_some());
        assert!(e.fix.is_some());
    }
}
