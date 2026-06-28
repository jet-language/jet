//! Driver-level API freeze — loads entry bundle and writes snapshot to disk.
//! Pure sema types and functions are re-exported from `Sema::ApiFreeze`.

use std::path::Path;

pub use crate::Sema::ApiFreeze::{
    api_cache_dir, fn_signature, load_all_snapshots, load_snapshot, project_capability_digest,
    save_snapshot, snapshot_from_items, ApiSnapshot, FrozenFn, API_SNAPSHOT_VERSION,
};

/// Freeze the public capability surface of `package` to disk for an
/// `api: stable|explicit` library target. Loads the entry bundle, resolves
/// D-CAP8 inference, snapshots the entry module's `pub fn`s, and writes
/// `<project_root>/.jet/cache/api/<package>.api`. Returns the count of
/// frozen public functions, or `None` if the entry couldn't be loaded.
pub fn write_api_snapshot_for_entry(
    project_root: &Path,
    entry_file: &str,
    package: &str,
    version: &str,
) -> Option<usize> {
    let mut bundle = crate::Loader::load_entry_with_overlay(entry_file, None, true).ok()?;
    crate::Sema::resolve_capabilities(&mut bundle);
    let entry = &bundle.modules[bundle.entry];
    let snap = snapshot_from_items(&entry.items, package, version);
    let count = snap.funcs.len();
    save_snapshot(project_root, &snap).ok()?;
    Some(count)
}
