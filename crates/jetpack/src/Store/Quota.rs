use super::*;
use std::io;

const DEFAULT_MAX_HANGAR_BYTES: u64 = 128 * 1024 * 1024 * 1024;
const HANGAR_MAX_BYTES_ENV: &str = "JETPACK_HANGAR_MAX_BYTES";
const HANGAR_MAX_BYTES_CONFIG: &str = "config/hangar-max-bytes";
const ADMISSION_RESERVE_BYTES: u64 = 1024 * 1024;

/// Reserve a small journal and metadata allowance with every admission.
/// Without this guard band, a tree that exactly fills the object budget could
/// still make the durable receipt and closure transaction exceed the ceiling.
pub(crate) fn admission_reservation(incoming_bytes: u64) -> u64 {
    incoming_bytes.saturating_add(ADMISSION_RESERVE_BYTES)
}

pub(crate) fn admission_size(path: &Path) -> io::Result<u64> {
    footprint(path, None)
}

/// Enforce the per-Hangar byte ceiling before a staged tree is published.
///
/// The footprint is a logical upper bound: file bytes are counted without
/// following symlinks and without deduplicating hardlinks. That can evict
/// earlier than physical allocation requires, but it cannot undercount disk
/// pressure. The incoming stage is excluded because its bytes are supplied as
/// `incoming_bytes` instead.
pub(crate) fn ensure_hangar_capacity(
    roots: &Roots,
    incoming_bytes: u64,
    excluded: Option<&Path>,
) -> io::Result<()> {
    let hangar = roots.hangar_dir();
    Ingest::ensure_real_directory(&hangar, "Hangar root")?;
    let limit = configured_limit(roots)?;
    if fits(&hangar, incoming_bytes, limit, excluded)? {
        return Ok(());
    }

    let live = super::live_roots_unlocked(roots)?;
    let _ = super::sweep_build_scratch(&hangar)?;
    let mut retired = BTreeSet::new();
    sweep_orphans(roots, &live, &retired)?;
    if fits(&hangar, incoming_bytes, limit, excluded)? {
        return Ok(());
    }

    let mut candidates = Vec::new();
    for entry in super::object_dirs(&hangar)? {
        let path = entry.path();
        let id = entry.file_name().to_string_lossy().into_owned();
        // A malformed record has no trustworthy age or reachability facts.
        // Leave it in place for the repair/quarantine path instead of making
        // quota enforcement guess that it is safe to delete.
        if super::malformed_object_reason(&path)?.is_some() {
            continue;
        }
        let Some(meta) = super::read_meta(&path) else {
            continue;
        };
        if super::is_live(&id, &meta, &live) {
            continue;
        }
        candidates.push(Candidate {
            id,
            path,
            last_used_at: meta.last_used_at,
        });
    }
    // Oldest known use goes first. Unknown timestamps are last because they
    // are less informative and therefore less safe to evict.
    candidates.sort_by(|left, right| {
        (
            left.last_used_at.is_none(),
            left.last_used_at.unwrap_or(u64::MAX),
            &left.id,
        )
            .cmp(&(
                right.last_used_at.is_none(),
                right.last_used_at.unwrap_or(u64::MAX),
                &right.id,
            ))
    });

    for candidate in candidates {
        super::Closure::tombstone_closure_record_unlocked(roots, &candidate.id)?;
        super::remove_hangar_node(&candidate.path)?;
        retired.insert(candidate.id);
        sweep_orphans(roots, &live, &retired)?;
        if fits(&hangar, incoming_bytes, limit, excluded)? {
            return Ok(());
        }
    }

    let used = footprint(&hangar, excluded)?;
    Err(io::Error::other(format!(
        "Hangar store limit exceeded: {used} bytes used + {incoming_bytes} bytes incoming exceeds {limit} bytes; no evictable objects remain"
    )))
}

#[derive(Debug)]
struct Candidate {
    id: String,
    path: PathBuf,
    last_used_at: Option<u64>,
}

fn configured_limit(roots: &Roots) -> io::Result<u64> {
    let value = if let Some(value) = std::env::var_os(HANGAR_MAX_BYTES_ENV) {
        value.to_string_lossy().into_owned()
    } else {
        let path = roots.root.join(HANGAR_MAX_BYTES_CONFIG);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "Hangar quota config is not a regular file: {}",
                        path.display()
                    ),
                ));
            }
            Ok(_) => fs::read_to_string(path)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(DEFAULT_MAX_HANGAR_BYTES);
            }
            Err(error) => return Err(error),
        }
    };
    let limit = value.trim().parse::<u64>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{HANGAR_MAX_BYTES_ENV} or {HANGAR_MAX_BYTES_CONFIG} must contain one positive byte count"
            ),
        )
    })?;
    if limit == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{HANGAR_MAX_BYTES_ENV} or {HANGAR_MAX_BYTES_CONFIG} must contain one positive byte count"
            ),
        ));
    }
    Ok(limit)
}

fn fits(
    hangar: &Path,
    incoming_bytes: u64,
    limit: u64,
    excluded: Option<&Path>,
) -> io::Result<bool> {
    let used = footprint(hangar, excluded)?;
    Ok(used
        .checked_add(incoming_bytes)
        .is_some_and(|total| total <= limit))
}

fn footprint(path: &Path, excluded: Option<&Path>) -> io::Result<u64> {
    if excluded.is_some_and(|excluded| path == excluded) {
        return Ok(0);
    }
    let metadata = fs::symlink_metadata(path)?;
    let file_type = metadata.file_type();
    if file_type.is_symlink() || metadata.is_file() {
        return Ok(metadata.len());
    }
    if !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "Hangar footprint contains an unsupported node: {}",
                path.display()
            ),
        ));
    }
    let mut total: u64 = 0;
    for entry in fs::read_dir(path)? {
        total = total
            .checked_add(footprint(&entry?.path(), excluded)?)
            .ok_or_else(|| io::Error::other("Hangar footprint overflowed"))?;
    }
    Ok(total)
}

fn sweep_orphans(
    roots: &Roots,
    live: &super::LiveRoots,
    retired: &BTreeSet<String>,
) -> io::Result<()> {
    let orphaned = super::collect_orphaned_canonical_objects(roots, live, retired)?;
    for object in &orphaned {
        super::remove_hangar_node(&object.path)?;
    }
    if !orphaned.is_empty() {
        super::sync_store_directory(&roots.hangar_dir().join(super::OBJECTS_DIR))?;
    }

    // Receipt deletion is safe only when every package record has readable,
    // valid metadata. A damaged record must not turn this bounded path into a
    // destructive guess; valid object eviction still proceeds above.
    let mut retained = live.receipts.clone();
    let mut uncertain = false;
    for entry in super::object_dirs(&roots.hangar_dir())? {
        let id = entry.file_name().to_string_lossy().into_owned();
        if retired.contains(&id) {
            continue;
        }
        let Some(meta) = super::read_meta(&entry.path()) else {
            uncertain = true;
            continue;
        };
        if !meta.receipt.is_empty() {
            if !super::valid_receipt_digest(&meta.receipt) {
                uncertain = true;
            } else {
                retained.insert(meta.receipt);
            }
        }
    }
    if !uncertain {
        let _ = super::sweep_receipts(&roots.hangar_dir(), &retained, true)?;
    }
    Ok(())
}
