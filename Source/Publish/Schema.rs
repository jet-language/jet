//! Driver-level schema snapshot writer — loads entry bundle and writes snapshots.
//! Pure types live in `Sema::Schema` and are re-exported from `Publish/mod.rs`.

use crate::Sema::Schema::{save_snapshot, snapshot_from_struct};

/// Write schema snapshots for all `#PublishedSchema` structs in the entry bundle.
/// Called during `jet registry publish`. Returns the number of snapshots written.
pub fn write_schema_snapshots_for_entry(
    project_root: &std::path::Path,
    entry_file: &str,
    version: &str,
) -> usize {
    let bundle = match crate::Loader::load_entry_with_overlay(entry_file, None, true) {
        Ok(b) => b,
        Err(_) => return 0,
    };
    let mut count = 0;
    for module in &bundle.modules {
        for item in &module.items {
            if let crate::AST::Item::Struct(s) = item {
                if s.is_published_schema {
                    let snap = snapshot_from_struct(s, version);
                    if save_snapshot(project_root, &snap).is_ok() {
                        count += 1;
                    }
                }
            }
        }
    }
    count
}
