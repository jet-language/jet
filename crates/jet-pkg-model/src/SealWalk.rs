//! Sealed-manifest tuple walk (D-JPK-VERIFYONCE1=A).
//!
//! The warm verification path is a stat walk over every node of a Hangar
//! object: (kind, relative path, inode, size, mtime) per file/symlink, sorted,
//! hashed. Like the canonical-archive walker in `Envelope`, an unoptimized
//! walk dominates once hashing is fast, so it lives in this opt-level=3 crate
//! (see `[profile.dev.package.jet-pkg-model]` at the workspace root).
//! `jetpack::Store::Seal` owns the seal format and decisions; this module owns
//! only the walk.

use std::fs;
use std::io;
use std::path::Path;

struct Tuple {
    relative: Vec<u8>,
    kind: u8,
    inode: u64,
    size: u64,
    mtime_ns: i128,
}

/// Stat-walk `path` and return `(node_count, "sha256-…")` over the sorted
/// identity-tuple table. Opens no content files.
pub fn tuple_table(path: &Path) -> io::Result<(usize, String)> {
    let mut tuples = Vec::new();
    collect_tuples(path, &mut Vec::new(), &mut tuples)?;
    tuples.sort_by(|left, right| {
        left.relative
            .cmp(&right.relative)
            .then_with(|| left.kind.cmp(&right.kind))
    });
    let mut table = Vec::new();
    for tuple in &tuples {
        table.push(tuple.kind);
        table.push(b'\t');
        append_hex(&mut table, &tuple.relative);
        table.extend_from_slice(
            format!("\t{}\t{}\t{}\n", tuple.inode, tuple.size, tuple.mtime_ns).as_bytes(),
        );
    }
    Ok((
        tuples.len(),
        format!("sha256-{}", crate::SHA256::sha256_hex(&table)),
    ))
}

fn collect_tuples(path: &Path, relative: &mut Vec<u8>, tuples: &mut Vec<Tuple>) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    let file_type = metadata.file_type();
    if file_type.is_symlink() || metadata.is_file() {
        tuples.push(Tuple {
            relative: relative.clone(),
            kind: if file_type.is_symlink() { b'l' } else { b'f' },
            inode: inode(&metadata),
            size: metadata.len(),
            mtime_ns: mtime_ns(&metadata),
        });
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "Hangar object contains an unsupported node: {}",
                path.display()
            ),
        ));
    }
    let mut entries = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by(|left, right| os_bytes(&left.file_name()).cmp(&os_bytes(&right.file_name())));
    for entry in entries {
        let old_len = relative.len();
        if old_len != 0 {
            relative.push(b'/');
        }
        relative.extend_from_slice(&os_bytes(&entry.file_name()));
        collect_tuples(&entry.path(), relative, tuples)?;
        relative.truncate(old_len);
    }
    Ok(())
}

fn append_hex(output: &mut Vec<u8>, bytes: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize]);
        output.push(HEX[(byte & 0x0f) as usize]);
    }
}

fn inode(metadata: &fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        metadata.ino()
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        metadata.file_index().unwrap_or_default()
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = metadata;
        0
    }
}

fn mtime_ns(metadata: &fs::Metadata) -> i128 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        return i128::from(metadata.mtime()) * 1_000_000_000 + i128::from(metadata.mtime_nsec());
    }
    #[cfg(not(unix))]
    metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| {
            i128::from(duration.as_secs()) * 1_000_000_000 + i128::from(duration.subsec_nanos())
        })
        .unwrap_or_default()
}

fn os_bytes(value: &std::ffi::OsStr) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;
        return value.as_bytes().to_vec();
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt as _;
        return value.encode_wide().flat_map(u16::to_be_bytes).collect();
    }
    #[cfg(not(any(unix, windows)))]
    value.to_string_lossy().as_bytes().to_vec()
}
