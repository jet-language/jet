#![allow(dead_code)]

// Keep one audited archive ABI kernel across every execution tier. The ordinary
// Jet package is the public authority; this include makes its internal kernel
// available to the compiler seam without adding a codec-crate dependency.
include!("../../../corelib/core.archive/pkgs/archive/src/lib.rs");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_archive_entry_fanout_is_rejected_before_materialization() {
        let entries = (0..=MAX_ENTRIES)
            .map(|index| (format!("entry-{index}"), Vec::new()))
            .collect::<Vec<_>>();
        assert!(zip_write_all(&entries).is_empty());
        assert!(tar_write_all(&entries).is_empty());
    }
}
