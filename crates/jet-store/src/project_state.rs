//! #2517: the per-workspace state kept in the workspace-root `.jet/`.
//!
//! Owner ruling 2026-09-30: the shared content-addressed store (records,
//! artifacts) stays in the machine cache; everything that belongs to one
//! workspace lives in ONE `.jet/` at the workspace root, never one per
//! package. Callers pass the workspace root (`Loader::selected_project_root`).
//! This module owns:
//!
//! - `.jet/last-run/<program digest>`: the store-log record key of the last
//!   run of a program, which explain-build shows and reuse reasons compare
//!   against;
//! - `.jet/receipts/<closure digest>`: the newest Receipt blob for one
//!   invocation closure (see `receipt.rs`);
//! - `.jet/stamps`: file stamps (size, modification and change time, inode)
//!   with the digest last computed for each file. A matching stamp skips
//!   rehashing; it never decides freshness on its own, because the digest it
//!   returns was computed from the same bytes.
//!
//! Deleting `.jet/` costs speed, never correctness: a missing or undecodable
//! file reads as empty.

use jet_foundation::RecordCodec::{
    encode_record, record_map, RecordReader, RecordSection, RecordValue,
};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

const LAST_RUN_SCHEMA: &str = "jet.last-run/v1";
const STAMPS_SCHEMA: &str = "jet.stamps/v1";
const SECTION_ENTRIES: u64 = 1;

/// The workspace-root `.jet/` folder.
pub fn state_dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".jet")
}

pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(&temporary, path)
}

fn last_run_path(workspace_root: &Path, program: &str) -> PathBuf {
    state_dir(workspace_root)
        .join("last-run")
        .join(jet_foundation::SHA256::sha256_hex(program.as_bytes()))
}

/// Record the store-log record key of the last run of `program`.
pub fn write_last_run(workspace_root: &Path, program: &str, record_key: &str) -> io::Result<()> {
    let bytes = encode_record(
        LAST_RUN_SCHEMA,
        &[RecordSection::new(
            SECTION_ENTRIES,
            vec![record_map([
                ("program", RecordValue::str(program)),
                ("record", RecordValue::str(record_key)),
            ])],
        )],
    )
    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    write_atomic(&last_run_path(workspace_root, program), &bytes)
}

/// The store-log record key of the last run of `program`, if recorded.
pub fn last_run(workspace_root: &Path, program: &str) -> Option<String> {
    let bytes = std::fs::read(last_run_path(workspace_root, program)).ok()?;
    let reader = RecordReader::open_schema(&bytes, LAST_RUN_SCHEMA).ok()?;
    let entry = reader.element(SECTION_ENTRIES, 0).ok()?;
    if entry.field("program").ok()?.as_str().ok()? != program {
        return None;
    }
    Some(entry.field("record").ok()?.as_str().ok()?.to_string())
}

/// Identity of one file's current bytes on disk, short of reading them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    size: u64,
    modified_ns: i64,
    changed_ns: i64,
    inode: u64,
}

impl Stamp {
    #[cfg(unix)]
    fn of(path: &Path) -> Option<Self> {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::metadata(path).ok()?;
        metadata.is_file().then(|| Self {
            size: metadata.size(),
            modified_ns: metadata.mtime() * 1_000_000_000 + metadata.mtime_nsec(),
            changed_ns: metadata.ctime() * 1_000_000_000 + metadata.ctime_nsec(),
            inode: metadata.ino(),
        })
    }

    /// Without change time and inode a stamp cannot rule out an in-place
    /// rewrite, so non-Unix hosts always hash.
    #[cfg(not(unix))]
    fn of(_path: &Path) -> Option<Self> {
        None
    }
}

/// The `.jet/stamps` table.
#[derive(Debug, Default)]
pub struct StampTable {
    entries: BTreeMap<String, (Stamp, String)>,
    changed: bool,
}

impl StampTable {
    pub fn load(project_root: &Path) -> Self {
        let mut table = Self::default();
        let Ok(bytes) = std::fs::read(state_dir(project_root).join("stamps")) else {
            return table;
        };
        let Ok(reader) = RecordReader::open_schema(&bytes, STAMPS_SCHEMA) else {
            return table;
        };
        let Ok(entries) = reader.elements(SECTION_ENTRIES) else {
            return table;
        };
        for entry in entries {
            let field = |name: &str| entry.field(name).and_then(|value| value.as_int());
            let (Ok(path), Ok(digest)) = (
                entry.field("path").and_then(|value| value.as_str().map(str::to_string)),
                entry.field("digest").and_then(|value| value.as_str().map(str::to_string)),
            ) else {
                continue;
            };
            let (Ok(size), Ok(modified_ns), Ok(changed_ns), Ok(inode)) =
                (field("size"), field("modified_ns"), field("changed_ns"), field("inode"))
            else {
                continue;
            };
            let stamp = Stamp {
                size: size as u64,
                modified_ns,
                changed_ns,
                inode: inode as u64,
            };
            table.entries.insert(path, (stamp, digest));
        }
        table
    }

    /// SHA-256 hex of the file at `path`, reusing the recorded digest when
    /// the file's stamp is unchanged. `None` when the file cannot be read.
    pub fn file_digest(&mut self, path: &Path) -> Option<String> {
        let key = path.to_string_lossy().into_owned();
        let stamp = Stamp::of(path);
        if let (Some(stamp), Some((recorded, digest))) = (stamp, self.entries.get(&key)) {
            if stamp == *recorded {
                return Some(digest.clone());
            }
        }
        let bytes = std::fs::read(path).ok()?;
        let digest = jet_foundation::SHA256::sha256_hex(&bytes);
        // Only a stamp taken before the read and still equal after it may be
        // stored, so a write racing the read is never remembered.
        if let Some(stamp) = stamp.filter(|stamp| Stamp::of(path) == Some(*stamp)) {
            self.entries.insert(key, (stamp, digest.clone()));
            self.changed = true;
        }
        Some(digest)
    }

    /// Persist the table when a lookup added or refreshed an entry.
    pub fn save(&self, project_root: &Path) -> io::Result<()> {
        if !self.changed {
            return Ok(());
        }
        let entries = self
            .entries
            .iter()
            .map(|(path, (stamp, digest))| {
                record_map([
                    ("path", RecordValue::str(path.as_str())),
                    ("digest", RecordValue::str(digest.as_str())),
                    ("size", RecordValue::Int(stamp.size as i64)),
                    ("modified_ns", RecordValue::Int(stamp.modified_ns)),
                    ("changed_ns", RecordValue::Int(stamp.changed_ns)),
                    ("inode", RecordValue::Int(stamp.inode as i64)),
                ])
            })
            .collect();
        let bytes = encode_record(STAMPS_SCHEMA, &[RecordSection::new(SECTION_ENTRIES, entries)])
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
        write_atomic(&state_dir(project_root).join("stamps"), &bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priors_and_stamps_round_trip_and_rehash_changed_files() {
        let root = std::env::temp_dir().join(format!("jet-project-state-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(last_run(&root, "main.jet"), None);
        write_last_run(&root, "main.jet", "abc").unwrap();
        assert_eq!(last_run(&root, "main.jet").as_deref(), Some("abc"));
        assert_eq!(last_run(&root, "../main.jet"), None);

        let file = root.join("input.txt");
        std::fs::write(&file, b"one").unwrap();
        let mut table = StampTable::load(&root);
        let first = table.file_digest(&file).unwrap();
        assert_eq!(first, jet_foundation::SHA256::sha256_hex(b"one"));
        table.save(&root).unwrap();

        let mut reloaded = StampTable::load(&root);
        assert_eq!(reloaded.file_digest(&file).as_deref(), Some(first.as_str()));
        // A rewrite changes size and change time, so the digest is recomputed.
        std::fs::write(&file, b"changed").unwrap();
        assert_eq!(
            reloaded.file_digest(&file),
            Some(jet_foundation::SHA256::sha256_hex(b"changed"))
        );
        assert_eq!(reloaded.file_digest(&root.join("absent")), None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
