//! Git-registry sparse index — the on-disk shape a `jet registry publish` writes and a
//! resolver reads (card c56, D-JPK-CACHE1=A / D-VERSION1=A).
//!
//! The registry is an ordinary git repo. Each package gets one append-only
//! `index/<name>/<name>.jsonl` file, one JSON line per published version —
//! the same sparse-index shape cargo/crates.io proved. No serde (I6): the
//! line is a fixed five-field object we hand-write and hand-parse.
//!
//! ```text
//! <registry>/index/<name>/<name>.jsonl
//! {"name":"textkit","version":"1.2.0","content_hash":"sha256-…","fingerprint":"sha256-…","yanked":false}
//! ```
//!
//! `content_hash` / `fingerprint` are the exact fields already on
//! `Lock::LockedPackage` — the index does not invent new hash names.

use std::io;
use std::path::{Path, PathBuf};

/// One published-version line in the sparse index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    pub name: String,
    pub version: String,
    /// D-CASTORE1=A source-tree hash (same field as `LockedPackage::content_hash`).
    pub content_hash: String,
    /// Plan fingerprint (same field as `LockedPackage::fingerprint`).
    pub fingerprint: String,
    /// D-VERSION1=A: a yanked version stays in the index (never deleted) but is
    /// hidden from new resolution.
    pub yanked: bool,
    /// c146 (D-PKGSIGN1) TOFU pin: the publisher's hex Ed25519 public key.
    /// Written **once**, on the first published version of a package; empty on
    /// later versions (which are checked against the pin). A later version that
    /// *does* carry a differing key signals a key rotation (a fetch-time warning).
    pub public_key: String,
    /// c146: base64 Ed25519 signature over `content_hash`. Empty when the
    /// publish was `--no-sign`.
    pub signature: String,
}

impl IndexEntry {
    /// Serialize to a single canonical JSON line (no trailing newline). The
    /// c146 `public_key`/`signature` fields are always present (empty string
    /// when absent) so the line shape stays fixed and parsers never guess.
    pub fn to_jsonl(&self) -> String {
        format!(
            "{{\"name\":{},\"version\":{},\"content_hash\":{},\"fingerprint\":{},\"yanked\":{},\"public_key\":{},\"signature\":{}}}",
            json_str(&self.name),
            json_str(&self.version),
            json_str(&self.content_hash),
            json_str(&self.fingerprint),
            if self.yanked { "true" } else { "false" },
            json_str(&self.public_key),
            json_str(&self.signature),
        )
    }

    /// Parse one index line. Returns `None` for a line missing `name`/`version`.
    /// `public_key`/`signature` default to empty (backward-compatible with
    /// index lines written before c146).
    pub fn parse_line(line: &str) -> Option<IndexEntry> {
        Some(IndexEntry {
            name: str_field(line, "name")?,
            version: str_field(line, "version")?,
            content_hash: str_field(line, "content_hash").unwrap_or_default(),
            fingerprint: str_field(line, "fingerprint").unwrap_or_default(),
            yanked: bool_field(line, "yanked"),
            public_key: str_field(line, "public_key").unwrap_or_default(),
            signature: str_field(line, "signature").unwrap_or_default(),
        })
    }
}

/// c146 TOFU pin: the public key a package is anchored to — the first recorded
/// non-empty `public_key` across its versions. `None` if no version pinned one.
pub fn pinned_public_key(entries: &[IndexEntry]) -> Option<String> {
    entries
        .iter()
        .map(|e| e.public_key.trim())
        .find(|k| !k.is_empty())
        .map(|k| k.to_string())
}

/// `<repo>/index/<name>/<name>.jsonl`.
pub fn index_entry_path(repo: &Path, name: &str) -> PathBuf {
    repo.join("index").join(name).join(format!("{name}.jsonl"))
}

/// Read every version line recorded for `name` (empty if the file is absent).
pub fn read_entries(repo: &Path, name: &str) -> Vec<IndexEntry> {
    let path = index_entry_path(repo, name);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(IndexEntry::parse_line)
        .collect()
}

/// Find the recorded line for one exact `name`+`version`, if any.
pub fn find_entry(repo: &Path, name: &str, version: &str) -> Option<IndexEntry> {
    read_entries(repo, name)
        .into_iter()
        .find(|e| e.version == version)
}

/// Append one version line, creating `index/<name>/<name>.jsonl` if missing.
pub fn write_index_entry(repo: &Path, entry: &IndexEntry) -> io::Result<()> {
    let path = index_entry_path(repo, &entry.name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut text = std::fs::read_to_string(&path).unwrap_or_default();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&entry.to_jsonl());
    text.push('\n');
    std::fs::write(&path, text)
}

/// D-VERSION1=A: flip `yanked` to `true` on the matching line **in place** —
/// the line is rewritten, never removed. Returns `true` if a non-yanked line
/// for `name`+`version` was found and flipped.
pub fn mark_yanked(repo: &Path, name: &str, version: &str) -> io::Result<bool> {
    let path = index_entry_path(repo, name);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(false);
    };
    let mut found = false;
    let mut out = String::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match IndexEntry::parse_line(line) {
            Some(mut e) if e.name == name && e.version == version && !e.yanked => {
                e.yanked = true;
                found = true;
                out.push_str(&e.to_jsonl());
            }
            // Non-matching (or already-yanked) lines keep their original bytes.
            _ => out.push_str(line),
        }
        out.push('\n');
    }
    if found {
        std::fs::write(&path, out)?;
    }
    Ok(found)
}

/// Fetch-side view: the versions of `name` that a resolver may still pick — the
/// recorded lines minus the yanked ones (D-VERSION1=A: a yank hides a version
/// from new resolution without freeing its number).
pub fn non_yanked_entries(repo: &Path, name: &str) -> Vec<IndexEntry> {
    read_entries(repo, name)
        .into_iter()
        .filter(|e| !e.yanked)
        .collect()
}

// ──────────────────────────────────────────────
// Minimal JSON (I6: no serde)
// ──────────────────────────────────────────────

/// Quote + escape a string as a JSON string literal.
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Read `"key":"value"` from a flat one-line object, honouring `\"`/`\\` escapes.
fn str_field(line: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\":\"");
    let start = line.find(&pat)? + pat.len();
    let mut out = String::new();
    let mut chars = line[start..].chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => out.push(other),
                None => break,
            },
            '"' => return Some(out),
            _ => out.push(c),
        }
    }
    None
}

/// Read a `"key":true|false` field (defaults to `false` if absent/unset).
fn bool_field(line: &str, key: &str) -> bool {
    let pat = format!("\"{key}\":");
    line.find(&pat)
        .map(|i| line[i + pat.len()..].trim_start().starts_with("true"))
        .unwrap_or(false)
}

// ──────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "jet_index_{}_{}",
            label,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn entry(name: &str, version: &str, yanked: bool) -> IndexEntry {
        IndexEntry {
            name: name.to_string(),
            version: version.to_string(),
            content_hash: "sha256-tree".to_string(),
            fingerprint: "sha256-fp".to_string(),
            yanked,
            public_key: String::new(),
            signature: String::new(),
        }
    }

    #[test]
    fn pin_is_first_nonempty_public_key() {
        let mut a = entry("textkit", "1.0.0", false);
        a.public_key = "aa".to_string();
        let b = entry("textkit", "1.1.0", false); // empty key
        let mut c = entry("textkit", "1.2.0", false);
        c.public_key = "bb".to_string();
        assert_eq!(pinned_public_key(&[a, b, c]), Some("aa".to_string()));
        assert_eq!(pinned_public_key(&[entry("x", "1.0.0", false)]), None);
    }

    #[test]
    fn jsonl_roundtrips() {
        let e = entry("textkit", "1.2.0", false);
        let line = e.to_jsonl();
        assert!(line.contains("\"name\":\"textkit\""));
        assert!(line.contains("\"yanked\":false"));
        assert_eq!(IndexEntry::parse_line(&line), Some(e));
    }

    #[test]
    fn write_appends_one_line_per_version() {
        let repo = scratch("append");
        write_index_entry(&repo, &entry("textkit", "1.0.0", false)).unwrap();
        write_index_entry(&repo, &entry("textkit", "1.1.0", false)).unwrap();
        let all = read_entries(&repo, "textkit");
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].version, "1.0.0");
        assert_eq!(all[1].version, "1.1.0");
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn find_entry_matches_exact_version() {
        let repo = scratch("find");
        write_index_entry(&repo, &entry("textkit", "1.0.0", false)).unwrap();
        assert!(find_entry(&repo, "textkit", "1.0.0").is_some());
        assert!(find_entry(&repo, "textkit", "9.9.9").is_none());
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn mark_yanked_flips_in_place_never_deletes() {
        let repo = scratch("yank");
        write_index_entry(&repo, &entry("textkit", "1.0.0", false)).unwrap();
        write_index_entry(&repo, &entry("textkit", "1.1.0", false)).unwrap();

        let flipped = mark_yanked(&repo, "textkit", "1.0.0").unwrap();
        assert!(flipped, "an existing version must flip");

        // Line count unchanged — a yank rewrites, never removes.
        let all = read_entries(&repo, "textkit");
        assert_eq!(all.len(), 2);
        assert!(all.iter().find(|e| e.version == "1.0.0").unwrap().yanked);
        assert!(!all.iter().find(|e| e.version == "1.1.0").unwrap().yanked);

        // A resolver now skips the yanked version.
        let live = non_yanked_entries(&repo, "textkit");
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].version, "1.1.0");
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn mark_yanked_missing_version_is_false() {
        let repo = scratch("yank_missing");
        write_index_entry(&repo, &entry("textkit", "1.0.0", false)).unwrap();
        assert!(!mark_yanked(&repo, "textkit", "2.0.0").unwrap());
        std::fs::remove_dir_all(&repo).ok();
    }
}
