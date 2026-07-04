//! Driver-level API freeze — loads entry bundle and writes snapshot to disk.
//! Pure sema types and functions are re-exported from `Sema::ApiFreeze`.

use std::path::Path;

pub use crate::Sema::ApiFreeze::{
    api_cache_dir, fn_signature, load_all_snapshots, load_snapshot, project_capability_digest,
    save_snapshot, snapshot_from_items, ApiSnapshot, FrozenFn, API_SNAPSHOT_VERSION,
};

/// Snapshot every publish's public-function surface to disk (pub-metadata
/// semver diffing, S2/D-MEM1 — the capability-tier freeze/elevation this used
/// to gate on is gone; signatures are decided at parse time and never drift).
/// Loads the entry bundle, snapshots the entry module's `pub fn`s, and writes
/// `<project_root>/.jet/cache/api/<package>.api`. Returns the count of
/// snapshotted public functions, or `None` if the entry couldn't be loaded.
pub fn write_api_snapshot_for_entry(
    project_root: &Path,
    entry_file: &str,
    package: &str,
    version: &str,
) -> Option<usize> {
    let bundle = crate::Loader::load_entry_with_overlay(entry_file, None, true).ok()?;
    let entry = &bundle.modules[bundle.entry];
    let snap = snapshot_from_items(&entry.items, package, version);
    let count = snap.funcs.len();
    save_snapshot(project_root, &snap).ok()?;
    Some(count)
}
