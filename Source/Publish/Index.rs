//! Git-registry sparse index — the on-disk shape a `jet registry publish` writes and a
//! resolver reads (card c56, D-JPK-CACHE1=A / D-VERSION1=A).
//!
//! The registry is an ordinary git repo. Each package gets one append-only
//! `index/<name>/<name>.jsonl` file, one JSON line per published version —
//! the same sparse-index shape cargo/crates.io proved. No serde (I6): the
//! line is a fixed seven-field object parsed by Jet's shared std-only JSON
//! parser; the two signing fields may be absent on legacy lines.
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
use jet_foundation::JSON::{json_escape, parse_json, JSONValue};

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

    /// Parse one index line. Returns `None` for malformed JSON, wrong field
    /// types, duplicate keys, or a line missing `name`/`version`.
    /// `public_key`/`signature` default to empty (backward-compatible with
    /// index lines written before c146).
    pub fn parse_line(line: &str) -> Option<IndexEntry> {
        let JSONValue::Object(fields) = parse_json(line).ok()? else {
            return None;
        };
        const KEYS: &[&str] = &[
            "name",
            "version",
            "content_hash",
            "fingerprint",
            "yanked",
            "public_key",
            "signature",
        ];
        if fields.keys().any(|key| !KEYS.contains(&key.as_str())) {
            return None;
        }
        Some(IndexEntry {
            name: required_string(&fields, "name")?,
            version: required_string(&fields, "version")?,
            content_hash: optional_string(&fields, "content_hash")?,
            fingerprint: optional_string(&fields, "fingerprint")?,
            yanked: optional_bool(&fields, "yanked")?,
            public_key: optional_string(&fields, "public_key")?,
            signature: optional_string(&fields, "signature")?,
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
pub fn read_entries(repo: &Path, name: &str) -> io::Result<Vec<IndexEntry>> {
    let path = index_entry_path(repo, name);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(line, raw)| {
            IndexEntry::parse_line(raw).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("malformed registry index record at line {}", line + 1),
                )
            })
        })
        .collect()
}

/// Find the recorded line for one exact `name`+`version`, if any.
pub fn find_entry(repo: &Path, name: &str, version: &str) -> io::Result<Option<IndexEntry>> {
    Ok(read_entries(repo, name)?
        .into_iter()
        .find(|e| e.version == version))
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
            Some(_) => out.push_str(line),
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "malformed registry index record",
                ))
            }
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
pub fn non_yanked_entries(repo: &Path, name: &str) -> io::Result<Vec<IndexEntry>> {
    Ok(read_entries(repo, name)?
        .into_iter()
        .filter(|e| !e.yanked)
        .collect())
}

// ──────────────────────────────────────────────
// Minimal JSON (I6: no serde)
// ──────────────────────────────────────────────

/// Quote + escape a string as a JSON string literal.
fn json_str(s: &str) -> String {
    format!("\"{}\"", json_escape(s))
}

fn required_string(
    fields: &std::collections::HashMap<String, JSONValue>,
    key: &str,
) -> Option<String> {
    match fields.get(key)? {
        JSONValue::String(value) if !value.is_empty() => Some(value.clone()),
        _ => None,
    }
}

fn optional_string(
    fields: &std::collections::HashMap<String, JSONValue>,
    key: &str,
) -> Option<String> {
    match fields.get(key) {
        None => Some(String::new()),
        Some(JSONValue::String(value)) => Some(value.clone()),
        _ => None,
    }
}

fn optional_bool(
    fields: &std::collections::HashMap<String, JSONValue>,
    key: &str,
) -> Option<bool> {
    match fields.get(key) {
        None => Some(false),
        Some(JSONValue::Bool(value)) => Some(*value),
        _ => None,
    }
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
        let all = read_entries(&repo, "textkit").unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].version, "1.0.0");
        assert_eq!(all[1].version, "1.1.0");
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn find_entry_matches_exact_version() {
        let repo = scratch("find");
        write_index_entry(&repo, &entry("textkit", "1.0.0", false)).unwrap();
        assert!(find_entry(&repo, "textkit", "1.0.0").unwrap().is_some());
        assert!(find_entry(&repo, "textkit", "9.9.9").unwrap().is_none());
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
        let all = read_entries(&repo, "textkit").unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().find(|e| e.version == "1.0.0").unwrap().yanked);
        assert!(!all.iter().find(|e| e.version == "1.1.0").unwrap().yanked);

        // A resolver now skips the yanked version.
        let live = non_yanked_entries(&repo, "textkit").unwrap();
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

    #[test]
    fn hostile_jsonl_is_rejected_and_never_treated_as_an_absent_version() {
        for line in [
            r#"{"outer":{"name":"fake"},"version":"1.0.0"}"#,
            r#"{"name":"x","name":"y","version":"1.0.0"}"#,
            r#"{"name":"x","version":1,"yanked":false}"#,
            r#"{"name":"x","version":"1.0.0","yanked":"false"}"#,
            r#"{"name":"x","version":"1.0.0","signature":"\q"}"#,
            r#"{"name":"x","version":"1.0.0","future_field":true}"#,
        ] {
            assert!(IndexEntry::parse_line(line).is_none(), "accepted: {line}");
        }

        let repo = scratch("malformed");
        let path = index_entry_path(&repo, "textkit");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{malformed}\n").unwrap();
        assert_eq!(
            read_entries(&repo, "textkit").unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            mark_yanked(&repo, "textkit", "1.0.0")
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        std::fs::remove_dir_all(&repo).ok();
    }
}
