//! Ratification enforcement (invariant I7 + docs/spec/syntax-decisions.md).
//!
//! Every `cargo test` run verifies that `docs/spec/syntax-decisions.md` and
//! `crates/jet-foundation/src/Syntax.rs` plus its split fragments stay in sync — ratified decisions
//! cannot drift back to "provisional" in code, and open/deferred decisions cannot land in syntax
//! without owner sign-off.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

#[test]
fn ratified_decisions_enforced() {
    let docs =
        fs::read_to_string("docs/spec/syntax-decisions.md").expect("docs/spec/syntax-decisions.md");
    let syntax = read_syntax_surface();
    let diag = fs::read_to_string("docs/spec/diagnostics.md").expect("docs/spec/diagnostics.md");

    let ratified = extract_section_ids(&docs, "## Ratified", "## Provisional");
    let open = extract_open_registry_ids(&docs);
    let deferred = BTreeSet::from(["S53", "S56"]);
    let staged = extract_staged_manifest(&docs);

    let syntax_entries = parse_syntax_rs_status(&syntax);
    let syntax_ids: BTreeSet<_> = syntax_entries.keys().cloned().collect();

    // Provisional table must not list real decision IDs.
    let provisional_table = extract_provisional_table_ids(&docs);
    assert!(
        provisional_table.is_empty(),
        "docs/spec/syntax-decisions.md Provisional table still lists {:?}; move to Ratified or delete the row",
        provisional_table
    );

    // Every syntax.rs decision ID must be ratified — not open or deferred.
    for id in &syntax_ids {
        assert!(
            ratified.contains(id.as_str()),
            "{id} is in crates/jet-foundation/src/Syntax.rs or Syntax/ fragments but not ratified in docs/spec/syntax-decisions.md Ratified section"
        );
        assert!(
            !open.contains(id.as_str()),
            "{id} is open in docs/spec/syntax-decisions.md but already present in crates/jet-foundation/src/Syntax.rs or Syntax/ fragments — ratify or remove"
        );
        assert!(
            !deferred.contains(id.as_str()),
            "{id} is deferred in docs/spec/syntax-decisions.md but present in crates/jet-foundation/src/Syntax.rs or Syntax/ fragments"
        );
    }

    // No ratified ID may remain marked provisional in syntax.rs.
    for (id, status) in &syntax_entries {
        if ratified.contains(id.as_str()) {
            assert_ne!(
                status.as_str(),
                "provisional",
                "{id} is ratified in docs/spec/syntax-decisions.md but still provisional in crates/jet-foundation/src/Syntax.rs or Syntax/ fragments"
            );
        }
    }

    // Surface-syntax ratified IDs must have at least one syntax.rs entry.
    const SURFACE_IN_SYNTAX_RS: &[&str] = &[
        "N1", "N2", "S1", "S2", "S3", "S5", "S6", "S7", "S8", "S9", "S10", "S11", "S13", "S16",
        "S17", "S18", "S19", "S20", "S22", "S23", "S24", "S27", "S29", "S30", "S32", "S34", "S35",
        "S36", "S46", "S55", "S57", "S59", "S76", "S80", "S82",
        "S84",
        // S81 (`?continue`) superseded by D-ORRETURN-CANON1=A — canonical form is `expr ?? next`
    ];
    for id in SURFACE_IN_SYNTAX_RS {
        if ratified.contains(*id) {
            assert!(
                syntax_ids.contains(*id),
                "ratified surface decision {id} must have an entry in crates/jet-foundation/src/Syntax.rs or Syntax/ fragments"
            );
        }
    }

    // Structural ratified decisions — enforced by parser/sema/tests, not constants.
    const STRUCTURAL_RATIFIED: &[&str] = &[
        "S4", "S12", "S14", "S15", "S21", "S31", "S33", "S37", "S38", "S28", "S39", "S40", "S41",
        "S42", "S43", "S44", "S45", "S46", "S47", "S48", "S49", "S50", "S26", "S55", "S57",
    ];
    for id in STRUCTURAL_RATIFIED {
        assert!(
            ratified.contains(*id),
            "structural decision {id} must stay ratified in docs/spec/syntax-decisions.md"
        );
    }

    // Staged ratified: pinned error codes must exist in docs/spec/diagnostics.md.
    for (id, code) in &staged {
        assert!(
            ratified.contains(id.as_str()),
            "staged entry {id} must be ratified"
        );
        assert!(
            diag.contains(&format!("| {code} |")),
            "staged decision {id} requires error code {code} in docs/spec/diagnostics.md"
        );
    }

    // S16 landed in M6 phase 3 — no longer staged.
    assert!(
        !staged.contains_key("S16"),
        "S16 must not remain staged after M6 phase 3"
    );
    assert!(
        !staged.contains_key("S7"),
        "S7 must not remain staged after M4"
    );
}

// ---------------------------------------------------------------------------
// I7 pin (card #447 / durability W2): every `pub const KW_*` / `SIGIL_*` in
// crates/jet-foundation/src/Syntax.rs and its Syntax/ fragments must sit
// directly under (or carry a trailing) comment naming a decision ID (S123 /
// N12 / U12 / D-XXXX / D-XXXX=A). A blank line between the decision comment
// and the const closes the old "no adjacent comment" escape (decisions.rs
// used to `continue` past any const whose comment lacked a status tag) — no
// more silent unratified keyword/sigil constants. Scope is KW_*/SIGIL_*
// (the user-typeable keyword/sigil surface I7 governs); other pub consts
// (types, attrs, aggregated tables) are covered by the broader
// ratified_decisions_enforced check above.
// ---------------------------------------------------------------------------
#[test]
fn every_syntax_const_has_adjacent_decision_comment() {
    let root = "crates/jet-foundation/src/Syntax";
    let mut files: Vec<String> = vec!["crates/jet-foundation/src/Syntax.rs".to_string()];
    let mut entries: Vec<_> = fs::read_dir(root)
        .expect(root)
        .map(|e| e.expect("Syntax fragment entry").path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("rs"))
        .collect();
    entries.sort();
    for path in entries {
        files.push(path.display().to_string());
    }

    // Ratchet (card #447 / W2): foundational keywords predating the
    // S-numbered decision log carry a `KW_DECISION_ID_EXEMPT` marker comment
    // instead of a fabricated decision ID. This count must never grow —
    // shrinking (by finding/assigning the real ID) is always welcome.
    const EXEMPT_MARKER: &str = "KW_DECISION_ID_EXEMPT";
    const EXEMPT_BASELINE: usize = 2; // KW_RETURN, KW_IT

    let mut violations = Vec::new();
    let mut exempt_count = 0usize;
    for file in &files {
        let source = fs::read_to_string(file).unwrap_or_else(|err| panic!("{file}: {err}"));
        let mut covered = false;
        let mut exempt = false;
        for (i, line) in source.lines().enumerate() {
            let t = line.trim();
            if t.is_empty() {
                covered = false;
                exempt = false;
                continue;
            }
            if t.starts_with("///") || t.starts_with("//!") {
                if line_has_decision_id(t) {
                    covered = true;
                }
                if t.contains(EXEMPT_MARKER) {
                    exempt = true;
                }
                continue;
            }
            let is_kw_or_sigil = t.starts_with("pub const KW_") || t.starts_with("pub const SIGIL_");
            if is_kw_or_sigil && !covered && !line_has_decision_id(t) {
                if exempt {
                    exempt_count += 1;
                } else {
                    violations.push(format!("{file}:{}: {t}", i + 1));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "pub const KW_*/SIGIL_* declarations without an adjacent decision-ID doc \
         comment (I7 — add `/// S123 (ratified): …` or `/// D-XXXX=A: …` directly \
         above, no blank line in between; or a `KW_DECISION_ID_EXEMPT` marker \
         comment if truly foundational and pre-dates the decision log):\n{}",
        violations.join("\n")
    );
    assert!(
        exempt_count <= EXEMPT_BASELINE,
        "KW_DECISION_ID_EXEMPT count grew from {EXEMPT_BASELINE} to {exempt_count} — \
         find/assign a real decision ID instead of exempting a new keyword const"
    );
}

/// #1670 / D-ONCE-LAW1=A: a real tombstone guard, not two hand-picked
/// sentences. Every truth has one home (docs/spec/syntax-decisions.md's
/// "Superseded & deferred IDs (tombstones)" section); a comment citing a
/// tombstoned ID is only honest if it says so. This parses the tombstone ID
/// list from that section (so a newly-tombstoned ID is covered automatically,
/// no hand-maintained copy) and scans every `.rs` comment under `crates/`
/// and `Source/` for a literal citation. A hit must carry a provenance word
/// (retires/retired/superseded/formerly/historical/tombstoned/predates)
/// in the same comment block, or it reads as current law and fails the test.
///
/// Scope note: only `D-`-prefixed tombstone IDs are scanned. Bare `S`/`N`/`U`
/// tombstone numbers (S6, S10, U1, U10, …) collide with unrelated numbering
/// used elsewhere in the corpus — e.g. `D-MEM1 S6` names a memory-model spec
/// section, not the retired semicolon decision S6 — so a literal-substring
/// scan over them produces false positives a lazy guard cannot tell apart
/// from a real citation. `D-` IDs are namespaced and collision-free, and they
/// are the entire evidence base for this card (~25 D-CTMARKER1 + ~20
/// D-MARKERMOVE* citations). Widening to bare S/N/U tombstones needs
/// contextual disambiguation, which is future work, not a silent allowlist.
///
/// The scan covers all of `crates/` and `Source/` with no exclusions — a
/// carved-out path is a hole the guard cannot see (review bd10-rev proved the
/// point by planting a violation in an excluded path).
#[test]
fn tombstoned_decision_ids_carry_provenance() {
    let docs =
        fs::read_to_string("docs/spec/syntax-decisions.md").expect("docs/spec/syntax-decisions.md");
    let tombstones = extract_tombstone_d_ids(&docs);
    assert!(
        tombstones.contains("D-CTMARKER1")
            && tombstones.contains("D-MARKERMOVE1")
            && tombstones.contains("D-MARKERMOVE2")
            && tombstones.contains("D-MARKERMOVE3")
            && tombstones.contains("D-MARKER-FAMILY1"),
        "tombstone extraction regressed: expected D-CTMARKER1/D-MARKER-FAMILY1/D-MARKERMOVE1..3, got {tombstones:?}"
    );

    const EXCLUDED_DIRS: &[&str] = &[];
    const EXCLUDED_FILES: &[&str] = &[];
    const PROVENANCE_WORDS: &[&str] = &[
        "retires",
        "retired",
        "superseded",
        "formerly",
        "historical",
        "tombstoned",
        "predates",
    ];

    let mut files = Vec::new();
    for root in ["crates", "Source"] {
        collect_rs_files(root, EXCLUDED_DIRS, &mut files);
    }

    let mut violations = Vec::new();
    for path in &files {
        if EXCLUDED_FILES.contains(&path.as_str()) {
            continue;
        }
        let raw = fs::read_to_string(path).unwrap_or_else(|err| panic!("{path}: {err}"));
        for (block_start, block) in comment_blocks(&raw) {
            let joined = block.join(" ");
            for id in &tombstones {
                if !contains_id(&joined, id) {
                    continue;
                }
                let has_provenance = PROVENANCE_WORDS
                    .iter()
                    .any(|w| joined.to_lowercase().contains(w));
                if !has_provenance {
                    violations.push(format!(
                        "{path}:{block_start}: cites tombstoned {id} with no provenance word ({})",
                        PROVENANCE_WORDS.join("/")
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "comments cite a tombstoned decision ID as if it were live law (#1670, D-ONCE-LAW1):\n{}",
        violations.join("\n")
    );
}

/// Recursively collect `.rs` file paths under `dir`, skipping any path equal
/// to (or nested under) an entry in `exclude_dirs`.
fn collect_rs_files(dir: &str, exclude_dirs: &[&str], out: &mut Vec<String>) {
    if exclude_dirs
        .iter()
        .any(|ex| dir == *ex || dir.starts_with(&format!("{ex}/")))
    {
        return;
    }
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut children: Vec<_> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    children.sort();
    for path in children {
        let path_str = path.display().to_string();
        if exclude_dirs
            .iter()
            .any(|ex| path_str == *ex || path_str.starts_with(&format!("{ex}/")))
        {
            continue;
        }
        if path.is_dir() {
            collect_rs_files(&path_str, exclude_dirs, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path_str);
        }
    }
}

/// Group contiguous `//`, `///`, `//!` comment lines into blocks (a blank
/// line or a non-comment line closes a block). Returns each block's
/// 1-indexed starting line number alongside its lines, so a phrase wrapped
/// across several comment lines is still checked as one continuous string.
fn comment_blocks(raw: &str) -> Vec<(usize, Vec<&str>)> {
    let mut blocks = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let mut current_start = 0usize;
    for (i, line) in raw.lines().enumerate() {
        let t = line.trim_start();
        if t.starts_with("//") {
            if current.is_empty() {
                current_start = i + 1;
            }
            current.push(line);
        } else if !current.is_empty() {
            blocks.push((current_start, std::mem::take(&mut current)));
        }
    }
    if !current.is_empty() {
        blocks.push((current_start, current));
    }
    blocks
}

/// Whole-word literal match of `id` inside `text`: the char immediately
/// before and after the match (if any) must not be alphanumeric or `-`, so
/// `S6` cannot false-match inside `S6-R` and `D-CTMARKER1` cannot false-match
/// inside a longer `D-CTMARKER1X`-shaped token.
fn contains_id(text: &str, id: &str) -> bool {
    let bytes = text.as_bytes();
    let idb = id.as_bytes();
    if idb.is_empty() || bytes.len() < idb.len() {
        return false;
    }
    for start in 0..=(bytes.len() - idb.len()) {
        if &bytes[start..start + idb.len()] != idb {
            continue;
        }
        let before_ok = start == 0 || {
            let c = bytes[start - 1];
            !(c.is_ascii_alphanumeric() || c == b'-')
        };
        let after = start + idb.len();
        let after_ok = after >= bytes.len() || {
            let c = bytes[after];
            !(c.is_ascii_alphanumeric() || c == b'-')
        };
        if before_ok && after_ok {
            return true;
        }
    }
    false
}

/// Parse `D-`-prefixed tombstone IDs out of the "Superseded & deferred IDs
/// (tombstones)" section of docs/spec/syntax-decisions.md. Each record is a
/// `**id-list [— title]**: description.` paragraph; the id-list is a
/// comma/` / `-separated list, and a compact `D-PREFIXn/m/o` spelling expands
/// to `D-PREFIXn`, `D-PREFIXm`, `D-PREFIXo`. Bare S/N/U tombstone numbers are
/// intentionally not extracted here — see the scope note on the test above.
fn extract_tombstone_d_ids(docs: &str) -> BTreeSet<String> {
    let section = section_between(
        docs,
        "### Superseded & deferred IDs (tombstones)",
        "## Enforcement",
    );
    let joined = section.replace('\n', " ");
    let mut ids = BTreeSet::new();
    let mut rest = joined.as_str();
    while let Some(start) = rest.find("**") {
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find("**") else {
            break;
        };
        let bold = &after_start[..end];
        let id_part = match bold.find(" — ") {
            Some(i) => &bold[..i],
            None => bold,
        };
        for comma_piece in id_part.split(',') {
            for piece in comma_piece.split(" / ") {
                let piece = piece.trim();
                if !piece.starts_with("D-") {
                    continue;
                }
                for expanded in expand_slash_compound(piece) {
                    ids.insert(expanded);
                }
            }
        }
        rest = &after_start[end + 2..];
    }
    ids
}

/// Expand a compact `D-PREFIXn/m/o` (or `D-PREFIXn/m-suffix`) tombstone
/// spelling into its full IDs. A token with no `/` is returned unchanged.
fn expand_slash_compound(tok: &str) -> Vec<String> {
    if !tok.contains('/') {
        return vec![tok.to_string()];
    }
    let segs: Vec<&str> = tok.split('/').collect();
    let first = segs[0];
    // Prefix: the non-digit head of the first segment, e.g. "D-CAP" out of "D-CAP1".
    let prefix_end = first
        .find(|c: char| c.is_ascii_digit())
        .unwrap_or(first.len());
    let prefix = &first[..prefix_end];
    // Suffix: any trailing non-digit text after the last segment's leading
    // digit run, e.g. "-words" out of "2-words" in `D-CAP1/2-words`. This
    // suffix belongs to every expanded ID, including the first.
    let last = segs[segs.len() - 1];
    let last_digit_end = last
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(last.len());
    let suffix = &last[last_digit_end..];
    let mut out = Vec::new();
    for seg in &segs {
        let digits: String = seg.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect();
        out.push(format!("{prefix}{digits}{suffix}"));
    }
    out
}

fn line_has_decision_id(line: &str) -> bool {
    // Matches S123 / N12 / U12 tokens, or D-XXXX / D-XXXX=A / D-XXXX1=A decision IDs.
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if (c == b'S' || c == b'N' || c == b'U')
            && i + 1 < bytes.len()
            && bytes[i + 1].is_ascii_digit()
        {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            let word_start_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
            let word_end_ok = j >= bytes.len() || !bytes[j].is_ascii_alphanumeric();
            if word_start_ok && word_end_ok {
                return true;
            }
            i = j;
            continue;
        }
        if c == b'D' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
            let word_start_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
            if word_start_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn read_syntax_surface() -> String {
    let root = "crates/jet-foundation/src/Syntax.rs";
    let mut syntax = fs::read_to_string(root).expect(root);
    let dir = "crates/jet-foundation/src/Syntax";
    let mut entries: Vec<_> = fs::read_dir(dir)
        .expect(dir)
        .map(|entry| entry.expect("Syntax fragment entry").path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("rs"))
        .collect();
    entries.sort();
    for path in entries {
        syntax.push('\n');
        syntax.push_str(&fs::read_to_string(&path).unwrap_or_else(|err| {
            panic!("{}: {err}", path.display());
        }));
    }
    syntax
}

fn extract_section_ids(docs: &str, start: &str, end: &str) -> BTreeSet<String> {
    let body = section_between(docs, start, end);
    ids_in_text(body)
}

fn extract_open_registry_ids(docs: &str) -> BTreeSet<String> {
    let body = section_between(docs, "### Registered for M3–M14", "## Decision log");
    ids_in_table_first_column(body)
}

fn extract_provisional_table_ids(docs: &str) -> BTreeSet<String> {
    let body = section_between(
        docs,
        "## Provisional — currently in the code",
        "## Open decisions",
    );
    ids_in_table_first_column(body)
        .into_iter()
        .filter(|id| id != "—")
        .collect()
}

fn extract_staged_manifest(docs: &str) -> BTreeMap<String, String> {
    let body = section_between(
        docs,
        "## Staged implementation",
        "## Provisional — currently in the code",
    );
    let mut out = BTreeMap::new();
    for line in body.lines() {
        let line = line.trim();
        if !line.starts_with('|') || line.starts_with("|---") || line.contains("ID") {
            continue;
        }
        let cols: Vec<_> = line
            .split('|')
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .collect();
        if cols.len() >= 4 {
            let id = cols[0].trim();
            let code = cols[3].trim();
            if (id.starts_with('S') || id.starts_with('N')) && code.starts_with('E') {
                out.insert(id.to_string(), code.to_string());
            }
        }
    }
    out
}

fn section_between<'a>(docs: &'a str, start: &str, end: &str) -> &'a str {
    let from = docs
        .find(start)
        .unwrap_or_else(|| panic!("docs/spec/syntax-decisions.md missing section header: {start}"));
    let rest = &docs[from + start.len()..];
    let to = rest.find(end).unwrap_or_else(|| {
        panic!("docs/spec/syntax-decisions.md missing section header after {start}: {end}")
    });
    &rest[..to]
}

fn ids_in_text(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in text.lines() {
        if let Some(id) = line_id(line) {
            out.insert(id);
        }
    }
    out
}

fn ids_in_table_first_column(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with('|') || line.starts_with("|---") {
            continue;
        }
        let cols: Vec<_> = line
            .split('|')
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .collect();
        if let Some(first) = cols.first() {
            let id = first.trim();
            if (id.starts_with('S') || id.starts_with('N')) && id.len() <= 4 {
                out.insert(id.to_string());
            }
        }
    }
    out
}

fn line_id(line: &str) -> Option<String> {
    let line = line.trim();
    if !line.starts_with("**") {
        return None;
    }
    let rest = &line[2..];
    let end = rest.find(' ').or_else(|| rest.find('\u{2014}'))?;
    let id = &rest[..end];
    // S/N decisions (language surface) and U decisions (unified ecosystem,
    // U1–U7) are enforced. D-JPK* IDs start with `D` and are left alone.
    if (id.starts_with('S') || id.starts_with('N') || id.starts_with('U'))
        && id[1..].chars().all(|c| c.is_ascii_digit())
    {
        Some(id.to_string())
    } else {
        None
    }
}

fn parse_syntax_rs_status(syntax: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in syntax.lines() {
        let line = line.trim();
        if !line.starts_with("///") {
            continue;
        }
        let rest = line.trim_start_matches('/').trim();
        if !rest.starts_with('S') && !rest.starts_with('N') && !rest.starts_with('U') {
            continue;
        }
        let id_end = rest
            .find(' ')
            .unwrap_or_else(|| panic!("malformed syntax.rs decision comment: {line}"));
        let id = &rest[..id_end];
        let status = if rest.contains("(provisional)") {
            "provisional"
        } else if rest.contains("(ratified") {
            "ratified"
        } else {
            continue;
        };
        out.insert(id.to_string(), status.to_string());
    }
    out
}
