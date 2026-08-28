use super::*;

/// Cleanup uses the project lock before the Hangar lock. Build publication has
/// the same order when it updates a project projection and then refreshes the
/// Hangar record; keeping one order prevents a concurrent lock update from
/// being published after cleanup has taken its reachability snapshot.
fn with_clean_locks<T>(
    roots: &Roots,
    action: impl FnOnce() -> std::io::Result<T>,
) -> std::io::Result<T> {
    match current_project_root()? {
        Some(project) => {
            crate::RuntimePolicy::with_project_lock(&project, "hangar-clean-project", || {
                crate::RuntimePolicy::with_lock(&roots.root, "hangar", action)
            })
        }
        None => crate::RuntimePolicy::with_lock(&roots.root, "hangar", action),
    }
}

pub(crate) fn live_roots_unlocked(roots: &Roots) -> std::io::Result<LiveRoots> {
    let cwd = std::env::current_dir()?;
    let graph = Closure::lifecycle_closure_graph_unlocked(roots)?;
    live_roots_from_graph(roots, &cwd, &graph)
}

#[cfg(test)]
pub(crate) fn live_roots_from(roots: &Roots, start: &Path) -> std::io::Result<LiveRoots> {
    let graph = Closure::lifecycle_closure_graph_unlocked(roots)?;
    live_roots_from_graph(roots, start, &graph)
}

fn live_roots_from_graph(
    roots: &Roots,
    start: &Path,
    graph: &Closure::ClosureGraph,
) -> std::io::Result<LiveRoots> {
    let mut live = lock_roots_from(start)?;
    let lifecycle = Lifecycle::protected_targets_unlocked(roots)?;
    let mut targets = live.output_hashes.clone();
    for (receipt, package) in &live.receipt_packages {
        let mut known = false;
        let mut matching = Vec::new();
        for record in graph.records.values().filter(|record| {
            parse_meta(&record.package_meta).is_some_and(|meta| meta.receipt == *receipt)
        }) {
            known = true;
            let meta = parse_meta(&record.package_meta).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "Hangar closure record `{}` has invalid package metadata for receipt `{receipt}`",
                        record.id
                    ),
                )
            })?;
            if meta.name == package.name
                && meta.version == package.version
                && receipt_source_matches(package, &meta.reference)
                && receipt_envelope_matches(package, &meta.envelope.output_hash)
            {
                matching.push((record, meta));
            }
        }
        // A receipt this Hangar has never issued belongs to a project served
        // by another root (multiple jetpack instances and hangars are the
        // intended design); it protects nothing here and must not fail clean.
        if !known {
            continue;
        }
        if matching.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "project lock receipt `{receipt}` disagrees with Hangar closure record: no matching package identity"
                ),
            ));
        }
        // A receipt is the lock's projection of the action root. Reachability
        // follows the same graph closure as output and lifecycle roots.
        for (record, _) in matching {
            targets.insert(record.primary.clone());
        }
    }
    for id in &live.ids {
        if let Some(record) = graph.records.get(id) {
            targets.insert(record.primary.clone());
        }
    }
    for record in graph.records.values() {
        let Some(meta) = parse_meta(&record.package_meta) else {
            continue;
        };
        if live
            .name_versions
            .contains(&(meta.name.clone(), meta.version.clone()))
        {
            targets.insert(record.primary.clone());
        }
    }
    targets.extend(lifecycle);
    for target in targets {
        live.output_hashes.extend(graph.closure(&target));
    }
    Ok(live)
}

fn current_project_root() -> std::io::Result<Option<PathBuf>> {
    let cwd = std::env::current_dir()?;
    let mut dir = Some(cwd.clone());
    while let Some(current) = dir {
        let managed = managed_dir(&current);
        match fs::symlink_metadata(&managed) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "project managed directory `{}` is not a real directory",
                        managed.display()
                    ),
                ));
            }
            Ok(_) => return Ok(Some(current)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                dir = current.parent().map(Path::to_path_buf);
            }
            Err(error) => return Err(error),
        }
    }
    // A project can be preparing its first lock. Serialize that preparation
    // against cleanup at the current root even before `.jet/lock` exists.
    // Filesystem root is the neutral cwd used by no-project commands; never
    // try to create `/.jet` there.
    let is_below_filesystem_root = cwd
        .parent()
        .is_some_and(|parent| !parent.as_os_str().is_empty() && parent != cwd.as_path());
    Ok(is_below_filesystem_root.then_some(cwd))
}

#[derive(Debug, Default)]
pub(crate) struct LiveRoots {
    ids: BTreeSet<String>,
    output_hashes: BTreeSet<String>,
    name_versions: BTreeSet<(String, String)>,
    pub(crate) receipts: BTreeSet<String>,
    receipt_packages: Vec<(String, crate::Lock::LockedPackage)>,
}

pub(crate) fn lock_roots_from(start: &Path) -> std::io::Result<LiveRoots> {
    let Some(lock_path) = nearest_lock_path(start)? else {
        return Ok(LiveRoots::default());
    };
    let raw = fs::read_to_string(&lock_path).map_err(|error| {
        std::io::Error::new(
            error.kind(),
            format!(
                "could not read project lock `{}`: {error}",
                lock_path.display()
            ),
        )
    })?;
    let lock = crate::Lock::parse(&raw).map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "could not parse project lock `{}`: {error}",
                lock_path.display()
            ),
        )
    })?;
    let mut roots = LiveRoots::default();
    for pkg in lock.packages {
        roots
            .name_versions
            .insert((pkg.name.clone(), pkg.version.clone()));
        if let Some(receipt) = pkg.receipt.clone() {
            if !valid_receipt_digest(&receipt) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "project lock `{}` contains an invalid Hangar receipt",
                        lock_path.display()
                    ),
                ));
            }
            roots.receipts.insert(receipt.clone());
            roots.receipt_packages.push((receipt, pkg.clone()));
        }
        if let Some(env) = pkg.envelope {
            if !env.output_hash.is_empty() {
                roots.output_hashes.insert(env.output_hash);
            }
        }
    }
    for toolchain in lock.toolchains {
        roots.ids.insert(toolchain.id);
        if !toolchain.envelope.output_hash.is_empty() {
            roots.output_hashes.insert(toolchain.envelope.output_hash);
        }
    }
    Ok(roots)
}

pub(crate) fn nearest_lock_path(start: &Path) -> std::io::Result<Option<PathBuf>> {
    let mut dir = Some(start);
    while let Some(current) = dir {
        let managed = managed_dir(current);
        match fs::symlink_metadata(&managed) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "project managed directory `{}` is a symlink; repair it before Hangar cleanup",
                        managed.display()
                    ),
                ));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "project managed directory `{}` is not a directory; repair it before Hangar cleanup",
                        managed.display()
                    ),
                ));
            }
            Ok(_) => {
                let lock = lock_path(current);
                match fs::symlink_metadata(&lock) {
                    Ok(metadata) if metadata.file_type().is_symlink() => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!(
                                "project lock `{}` is a symlink; repair it before Hangar cleanup",
                                lock.display()
                            ),
                        ));
                    }
                    Ok(metadata) if metadata.is_file() => return Ok(Some(lock)),
                    Ok(_) => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!(
                                "project lock `{}` is not a regular file; repair it before Hangar cleanup",
                                lock.display()
                            ),
                        ));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        dir = current.parent();
    }
    Ok(None)
}

pub(crate) fn is_live(id: &str, meta: &ParsedMeta, roots: &LiveRoots) -> bool {
    roots.receipts.contains(&meta.receipt)
        || roots.ids.contains(id)
        || (!meta.envelope.output_hash.is_empty()
            && roots.output_hashes.contains(&meta.envelope.output_hash))
        || (meta.envelope.output_hash.is_empty()
            && roots
                .name_versions
                .contains(&(meta.name.clone(), meta.version.clone())))
}

#[derive(Debug, Clone)]
pub(crate) struct MalformedObject {
    pub(crate) id: String,
    pub(crate) path: PathBuf,
    pub(crate) reason: &'static str,
}

pub(crate) fn malformed_object_reason(path: &Path) -> std::io::Result<Option<&'static str>> {
    let metadata_path = path.join("meta.json");
    match fs::symlink_metadata(&metadata_path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Ok(Some("metadata-not-file"))
        }
        Ok(metadata) if metadata.len() > MAX_GC_METADATA_BYTES => Ok(Some("metadata-too-large")),
        Ok(_) => {
            let Some(meta) = read_meta(path) else {
                return Ok(Some("malformed-metadata"));
            };
            if !meta.receipt.is_empty() && !valid_receipt_digest(&meta.receipt) {
                return Ok(Some("invalid-receipt"));
            }
            // A missing output makes this projection unusable. Probe through
            // the output link so a dangling symlink is recognized, but only
            // classify a proven ENOENT; permission and other I/O failures stay
            // fail-closed. Quarantine removes the record, never output bytes.
            if !meta.out.is_empty() {
                match fs::metadata(&meta.out) {
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        return Ok(Some("missing-output"));
                    }
                    Err(_) => {}
                }
            }
            Ok(None)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut entries = fs::read_dir(path)?;
            match entries.next() {
                None | Some(Ok(_)) => Ok(Some("missing-metadata")),
                Some(Err(error)) => Err(error),
            }
        }
        Err(error) => Err(error),
    }
}

fn malformed_objects(hangar: &Path) -> std::io::Result<Vec<MalformedObject>> {
    object_dirs(hangar)?
        .into_iter()
        .filter_map(|entry| {
            let path = entry.path();
            let id = entry.file_name().to_string_lossy().into_owned();
            match malformed_object_reason(&path) {
                Ok(Some(reason)) => Some(Ok(MalformedObject { id, path, reason })),
                Ok(None) => None,
                Err(error) => Some(Err(error)),
            }
        })
        .collect()
}

pub(crate) fn quarantine_malformed_objects(
    roots: &Roots,
    objects: &[MalformedObject],
) -> std::io::Result<usize> {
    if objects.is_empty() {
        return Ok(0);
    }
    let hangar = roots.hangar_dir();
    let quarantine = hangar.join("quarantine");
    let mut permissions = Ingest::MovePathPermissions::default();
    let result = (|| {
        permissions.make_writable(&hangar, &hangar)?;
        Ingest::ensure_real_directory(&quarantine, "Hangar quarantine")?;
        permissions.make_writable(&quarantine, &hangar)?;
        let mut moved = 0;
        for object in objects {
            // Admitted objects are directory, regular file, or symlink roots;
            // every shape is quarantinable by rename.
            match fs::symlink_metadata(&object.path) {
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            }
            permissions.make_writable(&object.path, &hangar)?;
            remove_seal(&object.path, &hangar)?;
            let sequence = GC_QUARANTINE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let destination = quarantine.join(format!(
                "gc-{}-{}-{}-{}",
                object.id,
                object.reason,
                now_secs(),
                sequence
            ));
            fs::rename(&object.path, &destination)?;
            permissions.renamed(&object.path, &destination);
            Closure::tombstone_closure_record_unlocked(roots, &object.id)?;
            moved += 1;
        }
        sync_store_directory(&quarantine)?;
        sync_store_directory(&hangar)?;
        Ok(moved)
    })();
    let restored = permissions.restore();
    match (result, restored) {
        (Ok(moved), Ok(())) => Ok(moved),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(restore)) => Err(std::io::Error::other(format!(
            "{error}; restoring Hangar permissions failed: {restore}"
        ))),
    }
}

fn retained_receipts(
    roots: &Roots,
    live: &LiveRoots,
    now: u64,
    retired: &BTreeSet<String>,
) -> std::io::Result<BTreeSet<String>> {
    let mut retained = live.receipts.clone();
    for ent in object_dirs(&roots.hangar_dir())? {
        let path = ent.path();
        let id = ent.file_name().to_string_lossy().into_owned();
        if retired.contains(&id) {
            continue;
        }
        if malformed_object_reason(&path)?.is_some() {
            continue;
        }
        let metadata_path = path.join("meta.json");
        match fs::symlink_metadata(&metadata_path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "Hangar metadata `{}` is not a regular file; repair it before receipt cleanup",
                        metadata_path.display()
                    ),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if fs::read_dir(&path)?.next().is_none() {
                    // Closure deletion tombstones the metadata file first and
                    // leaves an empty projection directory until cleanup.
                    // It is not a live object and must not block collection;
                    // non-empty unknown directories still fail closed below.
                    continue;
                }
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "Hangar object `{id}` has no metadata; repair it before receipt cleanup"
                    ),
                ));
            }
            Err(error) => return Err(error),
        }
        let meta = read_meta(&path).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Hangar object `{id}` has invalid metadata; repair it before receipt cleanup"
                ),
            )
        })?;
        let keep = is_live(&id, &meta, live)
            || meta.last_used_at.is_none()
            || now.saturating_sub(meta.last_used_at.unwrap_or(now)) < STALE_AFTER.as_secs();
        if keep && !meta.receipt.is_empty() {
            if !valid_receipt_digest(&meta.receipt) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Hangar object `{id}` has an invalid receipt digest"),
                ));
            }
            retained.insert(meta.receipt);
        }
    }
    Ok(retained)
}

#[derive(Debug, Clone)]
pub(crate) struct OrphanedCanonicalObject {
    pub(crate) path: PathBuf,
    bytes: u64,
}

pub(crate) fn collect_orphaned_canonical_objects(
    roots: &Roots,
    live: &LiveRoots,
    retired: &BTreeSet<String>,
) -> std::io::Result<Vec<OrphanedCanonicalObject>> {
    let graph = Closure::lifecycle_closure_graph_unlocked(roots)?;
    collect_orphaned_canonical_objects_with_graph(roots, live, retired, &graph)
}

fn collect_orphaned_canonical_objects_with_graph(
    roots: &Roots,
    live: &LiveRoots,
    retired: &BTreeSet<String>,
    graph: &Closure::ClosureGraph,
) -> std::io::Result<Vec<OrphanedCanonicalObject>> {
    let mut protected = live.output_hashes.clone();
    for (id, record) in &graph.records {
        if !retired.contains(id) {
            // A package owns every named output, not only its primary. Keep
            // this set aligned with the closure disk-usage path so GC cannot
            // unlink a live secondary output.
            for output in record.outputs.values() {
                protected.extend(graph.closure(output));
            }
        }
    }

    let objects = roots.hangar_dir().join(OBJECTS_DIR);
    let metadata = match fs::symlink_metadata(&objects) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Hangar object pool is not a real directory: {}",
                    objects.display()
                ),
            ));
        }
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let _ = metadata;
    let mut orphaned = Vec::new();
    for item in fs::read_dir(&objects)? {
        let item = item?;
        let path = item.path();
        let name = item.file_name().to_string_lossy().into_owned();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            // Seal admits a symlink as an object root. Account for the link
            // itself and never walk its target. Unknown names remain for
            // Hangar doctor rather than being removed by cleanup.
            if valid_receipt_digest(&name) && !protected.contains(&name) {
                orphaned.push(OrphanedCanonicalObject {
                    path,
                    bytes: metadata.len(),
                });
            }
            continue;
        }
        if metadata.is_dir() && valid_receipt_digest(&name) && !protected.contains(&name) {
            orphaned.push(OrphanedCanonicalObject {
                path,
                bytes: dir_size(&item.path()),
            });
        }
    }
    orphaned.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(orphaned)
}

pub(crate) fn remove_hangar_node(path: &Path) -> std::io::Result<()> {
    let is_object_node = path
        .parent()
        .and_then(|parent| parent.file_name())
        .is_some_and(|name| name == OBJECTS_DIR);
    if is_object_node {
        if let Some(hangar) = path.parent().and_then(Path::parent) {
            remove_seal(path, hangar)?;
        }
    }
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        if is_object_node {
            // Removing the directory entry is safe and does not follow the
            // symlink. Only direct canonical object roots reach this branch.
            return fs::remove_file(path);
        }
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("refusing to remove Hangar symlink `{}`", path.display()),
        ));
    }
    if metadata.is_dir() {
        make_tree_writable_for_removal(path)?;
        fs::remove_dir_all(path)
    } else if metadata.is_file() {
        let mut permissions = metadata.permissions();
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions)?;
        fs::remove_file(path)
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "refusing to remove unsupported Hangar node `{}`",
                path.display()
            ),
        ))
    }
}

pub(crate) fn sweep_receipts(
    hangar: &Path,
    retained: &BTreeSet<String>,
    apply: bool,
) -> std::io::Result<CleanReport> {
    let receipts = hangar.join(Closure::RECEIPTS_DIR);
    match fs::symlink_metadata(&receipts) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Hangar receipt directory is not a real directory: {}",
                    receipts.display()
                ),
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CleanReport::default())
        }
        Err(error) => return Err(error),
    }
    let mut report = CleanReport::default();
    let mut changed = false;
    for item in fs::read_dir(&receipts)? {
        let item = item?;
        let path = item.path();
        let name = item.file_name().to_string_lossy().into_owned();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Hangar receipt entry is a symlink: {}", path.display()),
            ));
        }
        if !valid_receipt_digest(&name) {
            continue;
        }
        if !metadata.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Hangar receipt `{name}` is not a regular file"),
            ));
        }
        if retained.contains(&name) {
            continue;
        }
        report.removed_receipts += 1;
        report.removed_receipt_bytes += metadata.len();
        if apply {
            fs::remove_file(&path)?;
            changed = true;
        }
    }
    if apply && changed {
        sync_store_directory(&receipts)?;
    }
    Ok(report)
}

/// D-JPK-GC1=B / U22: collect only unreferenced stale hangar objects, sweep
/// orphan transient and failed-build scratch, then optimize duplicate
/// Jet-owned files. Lockfile reachable entries and unknown legacy records are
/// retained.
pub fn clean_plan(roots: &Roots) -> std::io::Result<CleanReport> {
    let store = roots.hangar_dir();
    match fs::symlink_metadata(&store) {
        Ok(_) => Ingest::require_real_directory(&store, "Hangar root")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CleanReport::default())
        }
        Err(error) => return Err(error),
    }
    with_clean_locks(roots, || clean_plan_unlocked(roots))
}

fn clean_plan_unlocked(roots: &Roots) -> std::io::Result<CleanReport> {
    let store = roots.hangar_dir();
    let malformed = malformed_objects(&store)?;
    let malformed_ids = malformed
        .iter()
        .map(|object| object.id.clone())
        .collect::<BTreeSet<_>>();
    let graph = if malformed_ids.is_empty() {
        Closure::lifecycle_closure_graph_unlocked(roots)?
    } else {
        Closure::lifecycle_closure_graph_unlocked_ignoring(roots, &malformed_ids)?
    };
    let cwd = std::env::current_dir()?;
    let live = live_roots_from_graph(roots, &cwd, &graph)?;
    let mut report = sweep_build_scratch_plan(&store)?;
    report.quarantined_objects = malformed.len();
    let now = now_secs();
    let mut retired = malformed_ids;

    for ent in object_dirs(&store)? {
        let path = ent.path();
        let id = ent.file_name().to_string_lossy().into_owned();
        if malformed_object_reason(&path)?.is_some() {
            continue;
        }
        let Some(meta) = read_meta(&path) else {
            continue;
        };
        if is_live(&id, &meta, &live) || meta.last_used_at.is_none() {
            continue;
        }
        let last_used = meta.last_used_at.unwrap_or(now);
        if now.saturating_sub(last_used) < STALE_AFTER.as_secs() {
            continue;
        }
        retired.insert(id);
        report.removed_objects += 1;
        report.removed_bytes += dir_size(&path);
    }

    let orphaned = collect_orphaned_canonical_objects_with_graph(roots, &live, &retired, &graph)?;
    report.removed_objects += orphaned.len();
    report.removed_bytes += orphaned.iter().map(|node| node.bytes).sum::<u64>();

    let retained = retained_receipts(roots, &live, now, &retired)?;
    let receipts = sweep_receipts(&store, &retained, false)?;
    report.removed_receipts += receipts.removed_receipts;
    report.removed_receipt_bytes += receipts.removed_receipt_bytes;

    let opt = optimize_hangar_plan(&store)?;
    report.optimized_files += opt.optimized_files;
    report.optimized_bytes += opt.optimized_bytes;
    let cas = optimize_objects_cas_pool_plan(roots)?;
    report.optimized_files += cas.optimized_files;
    report.optimized_bytes += cas.optimized_bytes;
    Ok(report)
}

pub fn clean(roots: &Roots) -> std::io::Result<CleanReport> {
    with_clean_locks(roots, || clean_unlocked(roots))
}

fn clean_unlocked(roots: &Roots) -> std::io::Result<CleanReport> {
    let store = roots.hangar_dir();
    Ingest::ensure_real_directory(&store, "Hangar root")?;
    let malformed = malformed_objects(&store)?;
    let quarantined_objects = quarantine_malformed_objects(roots, &malformed)?;
    let graph = Closure::lifecycle_closure_graph_unlocked(roots)?;
    let cwd = std::env::current_dir()?;
    let live = live_roots_from_graph(roots, &cwd, &graph)?;
    let mut report = sweep_build_scratch(&store)?;
    report.quarantined_objects = quarantined_objects;
    let now = now_secs();
    let mut retired = BTreeSet::new();

    for ent in object_dirs(&store)? {
        let path = ent.path();
        let id = ent.file_name().to_string_lossy().into_owned();
        let Some(meta) = read_meta(&path) else {
            continue;
        };
        if is_live(&id, &meta, &live) || meta.last_used_at.is_none() {
            continue;
        }
        let last_used = meta.last_used_at.unwrap_or(now);
        if now.saturating_sub(last_used) < STALE_AFTER.as_secs() {
            continue;
        }
        let bytes = dir_size(&path);
        retired.insert(id.clone());
        Closure::tombstone_closure_record_unlocked(roots, &id)?;
        remove_seal(&path, &store)?;
        fs::remove_dir_all(&path)?;
        report.removed_objects += 1;
        report.removed_bytes += bytes;
    }

    let orphaned = collect_orphaned_canonical_objects_with_graph(roots, &live, &retired, &graph)?;
    for node in &orphaned {
        remove_hangar_node(&node.path)?;
        report.removed_objects += 1;
        report.removed_bytes += node.bytes;
    }
    if !orphaned.is_empty() {
        sync_store_directory(&store.join(OBJECTS_DIR))?;
    }

    let retained = retained_receipts(roots, &live, now, &retired)?;
    let receipts = sweep_receipts(&store, &retained, true)?;
    report.removed_receipts += receipts.removed_receipts;
    report.removed_receipt_bytes += receipts.removed_receipt_bytes;

    let opt = optimize_hangar(roots)?;
    report.optimized_files += opt.optimized_files;
    report.optimized_bytes += opt.optimized_bytes;
    Ok(report)
}

pub fn maybe_auto_clean(roots: &Roots) -> std::io::Result<Option<CleanReport>> {
    with_clean_locks(roots, || {
        let hangar = roots.hangar_dir();
        Ingest::ensure_real_directory(&hangar, "Hangar root")?;
        let stamp = hangar.join(AUTO_CLEAN_STAMP);
        let now = SystemTime::now();
        if std::env::var_os("JETPACK_AUTO_CLEAN_ALWAYS").is_none() {
            if let Ok(meta) = fs::metadata(&stamp) {
                if let Ok(modified) = meta.modified() {
                    if now.duration_since(modified).unwrap_or_default() < AUTO_CLEAN_AFTER {
                        return Ok(None);
                    }
                }
            }
        }
        let report = clean_unlocked(roots)?;
        let _ = fs::write(stamp, now_secs().to_string());
        Ok(Some(report))
    })
}

fn sweep_build_scratch_plan(hangar: &Path) -> std::io::Result<CleanReport> {
    let mut report = CleanReport::default();
    for scratch_name in [BUILD_SCRATCH_DIR, FAILED_SCRATCH_DIR] {
        let root = hangar.join(scratch_name);
        let label = if scratch_name == BUILD_SCRATCH_DIR {
            "Hangar build scratch"
        } else {
            "Hangar failed-build scratch"
        };
        match fs::symlink_metadata(&root) {
            Ok(_) => Ingest::require_real_directory(&root, label)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        }
        for ent in fs::read_dir(&root)? {
            let ent = ent?;
            let path = ent.path();
            if fs::symlink_metadata(&path)?.file_type().is_symlink() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("{label} entry is a symlink: {}", path.display()),
                ));
            }
            if scratch_name == BUILD_SCRATCH_DIR
                && crate::Provider::active_tmp_marker_is_live(&path)
            {
                continue;
            }
            report.swept_tmp += 1;
            report.swept_tmp_bytes += dir_size(&path);
        }
    }
    Ok(report)
}

pub(crate) fn sweep_build_scratch(hangar: &Path) -> std::io::Result<CleanReport> {
    let mut report = CleanReport::default();
    for scratch_name in [BUILD_SCRATCH_DIR, FAILED_SCRATCH_DIR] {
        let root = hangar.join(scratch_name);
        let label = if scratch_name == BUILD_SCRATCH_DIR {
            "Hangar build scratch"
        } else {
            "Hangar failed-build scratch"
        };
        match fs::symlink_metadata(&root) {
            Ok(_) => Ingest::require_real_directory(&root, label)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        }
        for ent in fs::read_dir(&root)? {
            let ent = ent?;
            let path = ent.path();
            if fs::symlink_metadata(&path)?.file_type().is_symlink() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("{label} entry is a symlink: {}", path.display()),
                ));
            }
            if scratch_name == BUILD_SCRATCH_DIR
                && crate::Provider::active_tmp_marker_is_live(&path)
            {
                continue;
            }
            let bytes = dir_size(&path);
            if fs::symlink_metadata(&path)?.is_dir() {
                fs::remove_dir_all(&path)?;
            } else {
                fs::remove_file(&path)?;
            }
            report.swept_tmp += 1;
            report.swept_tmp_bytes += bytes;
        }
    }
    Ok(report)
}

fn optimize_hangar_plan(hangar: &Path) -> std::io::Result<CleanReport> {
    let mut seen: BTreeMap<(u64, String), PathBuf> = BTreeMap::new();
    let mut report = CleanReport::default();
    for obj in object_dirs(hangar)? {
        for file in files_under(&obj.path()) {
            if file.file_name().and_then(|n| n.to_str()) == Some("meta.json") {
                continue;
            }
            let Ok(meta) = fs::metadata(&file) else {
                continue;
            };
            if !meta.is_file() || meta.len() == 0 {
                continue;
            }
            let Ok(bytes) = fs::read(&file) else { continue };
            let key = (meta.len(), SHA256::sha256_hex(&bytes));
            if seen.contains_key(&key) {
                report.optimized_files += 1;
                report.optimized_bytes += meta.len();
            } else {
                seen.insert(key, file);
            }
        }
    }
    Ok(report)
}

/// Read-only counterpart to [`optimize_objects_cas_pool`]. Keep the plan
/// honest for Store-v2 objects: `clean` applies the CAS pass even when there
/// are no legacy package directories at the Hangar root.
fn optimize_objects_cas_pool_plan(roots: &Roots) -> std::io::Result<CleanReport> {
    let hangar = roots.hangar_dir();
    let objects = hangar.join(OBJECTS_DIR);
    let objects_metadata = match fs::symlink_metadata(&objects) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CleanReport::default())
        }
        Err(error) => return Err(error),
    };
    if objects_metadata.file_type().is_symlink() || !objects_metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "Hangar object pool is not a real directory: {}",
                objects.display()
            ),
        ));
    }

    let cas = roots.shared_cas_dir();
    match fs::symlink_metadata(&cas) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("shared CAS pool is not a real directory: {}", cas.display()),
            ))
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }

    let mut object_dirs = Vec::new();
    for ent in fs::read_dir(&objects)? {
        let ent = ent?;
        let path = ent.path();
        let name = ent.file_name().to_string_lossy().into_owned();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            // Object-root symlinks are legal and have no files to share. Do
            // not follow or reject them while optimizing adjacent directories.
            continue;
        }
        if metadata.is_dir() && !name.ends_with(PARTIAL_SUFFIX) {
            object_dirs.push(path);
        }
    }

    let mut report = CleanReport::default();
    for object_dir in object_dirs {
        for file in files_under(&object_dir) {
            let metadata = fs::symlink_metadata(&file)?;
            if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() == 0 {
                continue;
            }
            let bytes = fs::read(&file)?;
            let mode = permission_identity(&metadata) & !0o222;
            let digest = Ingest::shared_cas_key(&SHA256::sha256_hex(&bytes), mode);
            let cas_file = cas.join(&digest);
            match fs::symlink_metadata(&cas_file) {
                Ok(existing) if existing.file_type().is_symlink() || !existing.is_file() => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!(
                            "Hangar CAS entry is not a regular file: {}",
                            cas_file.display()
                        ),
                    ));
                }
                Ok(existing) => {
                    if existing.len() != metadata.len()
                        || permission_identity(&existing) != mode
                        || fs::read(&cas_file)? != bytes
                    {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("Hangar CAS entry is corrupt: {}", cas_file.display()),
                        ));
                    }
                    if !same_file_inode(&file, &cas_file) {
                        report.optimized_files += 1;
                        report.optimized_bytes += metadata.len();
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    report.optimized_files += 1;
                    report.optimized_bytes += metadata.len();
                }
                Err(error) => return Err(error),
            }
        }
    }
    Ok(report)
}

fn optimize_hangar(roots: &Roots) -> std::io::Result<CleanReport> {
    let hangar = roots.hangar_dir();
    let mut report = optimize_package_tree_hardlinks(&hangar)?;
    let cas = optimize_objects_cas_pool(roots)?;
    report.optimized_files += cas.optimized_files;
    report.optimized_bytes += cas.optimized_bytes;
    Ok(report)
}

/// Legacy package-dir hardlink dedupe (pre-objects/ layout).
fn optimize_package_tree_hardlinks(hangar: &Path) -> std::io::Result<CleanReport> {
    let mut seen: BTreeMap<(u64, String), PathBuf> = BTreeMap::new();
    let mut report = CleanReport::default();
    for obj in object_dirs(hangar)? {
        for file in files_under(&obj.path()) {
            if file.file_name().and_then(|n| n.to_str()) == Some("meta.json") {
                continue;
            }
            let Ok(meta) = fs::metadata(&file) else {
                continue;
            };
            if !meta.is_file() || meta.len() == 0 {
                continue;
            }
            let Ok(bytes) = fs::read(&file) else { continue };
            let key = (meta.len(), SHA256::sha256_hex(&bytes));
            if let Some(first) = seen.get(&key) {
                if hardlink_replace(first, &file).is_ok() {
                    report.optimized_files += 1;
                    report.optimized_bytes += meta.len();
                }
            } else {
                seen.insert(key, file);
            }
        }
    }
    Ok(report)
}

/// Store v2: share immutable object files through the same CAS pool used by
/// native registration. After optimize, verify accepts those peers while
/// outside-hangar hardlinks still reject.
fn optimize_objects_cas_pool(roots: &Roots) -> std::io::Result<CleanReport> {
    let hangar = roots.hangar_dir();
    let objects = hangar.join(OBJECTS_DIR);
    Ingest::ensure_real_directory(&hangar, "Hangar root")?;
    Ingest::ensure_real_directory(&objects, "Hangar object pool")?;
    let mut object_dirs = Vec::new();
    for ent in fs::read_dir(&objects)? {
        let ent = ent?;
        let path = ent.path();
        let name = ent.file_name().to_string_lossy().into_owned();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            // Object-root symlinks are legal and have no files to share. Do
            // not follow or reject them while optimizing adjacent directories.
            continue;
        }
        if !metadata.is_dir() || name.ends_with(PARTIAL_SUFFIX) {
            continue;
        }
        object_dirs.push(path);
    }
    Ingest::with_shared_cas_lock(roots, || {
        let mut report = CleanReport::default();
        for path in object_dirs {
            let (optimized_files, optimized_bytes) =
                Ingest::share_tree_files_unlocked_with_report(roots, &path, false)?;
            report.optimized_files += optimized_files;
            report.optimized_bytes += optimized_bytes;
        }
        fs::File::open(&objects)?.sync_all()?;
        Ok(report)
    })
}

#[cfg(unix)]
fn permission_identity(meta: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    meta.permissions().mode() & 0o7777
}

#[cfg(not(unix))]
fn permission_identity(meta: &fs::Metadata) -> u32 {
    u32::from(meta.permissions().readonly())
}

fn same_file_inode(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let Ok(ma) = fs::metadata(a) else {
            return false;
        };
        let Ok(mb) = fs::metadata(b) else {
            return false;
        };
        ma.dev() == mb.dev() && ma.ino() == mb.ino()
    }
    #[cfg(not(unix))]
    {
        let _ = (a, b);
        false
    }
}

/// Run the cas-pool hardlink optimizer (also invoked from `clean`).
pub fn optimize_cas_pool(roots: &Roots) -> std::io::Result<CleanReport> {
    crate::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        optimize_objects_cas_pool(roots)
    })
}

/// Re-hash a hangar object with cas-peer hardlink law (hangar-internal OK).
pub fn verify_hangar_object(roots: &Roots, entry: &StoreEntry) -> Result<(), IngestError> {
    let _lock = crate::RuntimePolicy::acquire_lock(&roots.root, "hangar")
        .map_err(|error| IngestError::IO(error.to_string()))?;
    verify_hangar_object_unlocked(roots, entry)
}

pub(super) fn verify_hangar_object_unlocked(
    roots: &Roots,
    entry: &StoreEntry,
) -> Result<(), IngestError> {
    let hangar = roots.hangar_dir();
    let allow = !entry.platform_artifact_kind.is_empty();
    let graph = Closure::lifecycle_closure_graph_unlocked(roots)
        .map_err(|error| IngestError::Invalid(error.to_string()))?;
    let record = graph.records.get(&entry.id).ok_or_else(|| {
        IngestError::Invalid(format!("closure graph has no record `{}`", entry.id))
    })?;
    if record.primary != entry.envelope.output_hash
        || record.action_key != entry_action_key(entry)
        || record.references != entry.references.iter().cloned().collect()
    {
        return Err(IngestError::Invalid(format!(
            "closure graph disagrees with record `{}`",
            entry.id
        )));
    }
    let mut expected_outputs = entry.named_outputs.clone();
    expected_outputs.insert("out".to_string(), entry.envelope.output_hash.clone());
    if record.outputs != expected_outputs {
        return Err(IngestError::Invalid(format!(
            "closure graph output map disagrees with record `{}`",
            entry.id
        )));
    }
    for (name, expected) in &expected_outputs {
        let object = graph.objects.get(expected).ok_or_else(|| {
            IngestError::Invalid(format!(
                "closure graph output `{name}` is missing `{expected}`"
            ))
        })?;
        let digest = Ingest::full_verified_output_hash(Path::new(&object.path), &hangar, allow)
            .map_err(|error| IngestError::Invalid(error.to_string()))?;
        if &digest != expected {
            return Err(IngestError::Invalid(format!(
                "output `{name}` records `{expected}`, re-hash produced `{digest}`"
            )));
        }
    }
    if let Some(missing) = record
        .references
        .iter()
        .find(|digest| !graph.objects.contains_key(*digest))
    {
        return Err(IngestError::Invalid(format!(
            "closure record `{}` references missing object `{missing}`",
            entry.id
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HangarDoctorFinding {
    pub kind: String,
    pub subject: String,
    pub detail: String,
    pub fixed: bool,
    repair: DoctorRepair,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HangarDoctorReport {
    pub objects: usize,
    pub seal_reused: usize,
    pub seal_resealed: usize,
    pub seal_reseal_needed: usize,
    pub findings: Vec<HangarDoctorFinding>,
}

impl HangarDoctorReport {
    pub fn fixed_count(&self) -> usize {
        self.findings.iter().filter(|finding| finding.fixed).count()
    }

    pub fn remaining_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|finding| !finding.fixed)
            .count()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DoctorRepair {
    None,
    Remove(PathBuf),
    Refresh { path: PathBuf, digest: String },
}

struct DoctorScan {
    report: HangarDoctorReport,
    repair_paths: BTreeMap<String, String>,
}

/// Inspect the Hangar without taking a lock, replaying a journal, or creating
/// a missing store. `repair` is the explicit mutation boundary; it reuses the
/// native Nix admission path so a corrupt object is replaced only by bytes
/// that pass the signed cache verification already used during admission.
pub fn hangar_doctor(
    roots: &Roots,
    repair: bool,
    offline: bool,
) -> std::io::Result<HangarDoctorReport> {
    if !repair {
        return Ok(scan_hangar_doctor(roots, false)?.report);
    }

    crate::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        let mut scan = scan_hangar_doctor(roots, true)?;
        let hangar = roots.hangar_dir();
        let mut requests = BTreeMap::new();
        let mut request_index = 0usize;
        for finding in &scan.report.findings {
            let DoctorRepair::Refresh { path, digest } = &finding.repair else {
                continue;
            };
            NixCache::invalidate_verified_digest(path);
            let Some(store_path) = scan.repair_paths.get(digest) else {
                continue;
            };
            if !requests.contains_key(store_path) {
                let name = format!("doctor-{request_index}");
                requests.insert(
                    store_path.clone(),
                    NixOutputRequest {
                        name,
                        store_path: store_path.clone(),
                    },
                );
                request_index += 1;
            }
        }

        let mut repair_errors = BTreeMap::new();
        for (store_path, request) in requests {
            if let Err(error) = admit_nix_closure_with_progress(roots, &[request], offline, None) {
                repair_errors.insert(store_path, error.to_string());
            }
        }

        let mut post_repair_reused = 0;
        let mut post_repair_resealed = 0;
        let mut post_repair_needed = 0;
        for finding in &mut scan.report.findings {
            let action = finding.repair.clone();
            match action {
                DoctorRepair::None => {}
                DoctorRepair::Remove(path) => match remove_doctor_node(&path) {
                    Ok(()) => {
                        finding.fixed = true;
                        finding.detail = "removed".to_string();
                    }
                    Err(error) => {
                        finding.detail = format!("could not remove: {error}");
                    }
                },
                DoctorRepair::Refresh { path, digest } => {
                    let Some(store_path) = scan.repair_paths.get(&digest) else {
                        finding.detail = "no native binary-cache provenance".to_string();
                        continue;
                    };
                    if let Some(error) = repair_errors.get(store_path) {
                        finding.detail = format!("cache repair failed: {error}");
                        continue;
                    }
                    match doctor_object_digest(&path, &hangar, true) {
                        Ok((actual, status)) if actual == digest => {
                            post_repair_reused += usize::from(status.reused);
                            post_repair_resealed += usize::from(status.resealed);
                            post_repair_needed += usize::from(status.reseal_needed);
                            finding.fixed = true;
                            finding.detail = "repaired from native binary cache".to_string();
                        }
                        Ok((actual, status)) => {
                            post_repair_reused += usize::from(status.reused);
                            post_repair_resealed += usize::from(status.resealed);
                            post_repair_needed += usize::from(status.reseal_needed);
                            finding.detail = format!("repair left digest {actual}");
                        }
                        Err(error) => {
                            finding.detail = format!("repair could not verify object: {error}");
                        }
                    }
                }
            }
        }

        scan.report.seal_reused += post_repair_reused;
        scan.report.seal_resealed += post_repair_resealed;
        scan.report.seal_reseal_needed += post_repair_needed;

        Ok(scan.report)
    })
}

fn scan_hangar_doctor(roots: &Roots, write_seals: bool) -> std::io::Result<DoctorScan> {
    let hangar = roots.hangar_dir();
    let mut scan = DoctorScan {
        report: HangarDoctorReport::default(),
        repair_paths: BTreeMap::new(),
    };
    match fs::symlink_metadata(&hangar) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            scan.report.findings.push(HangarDoctorFinding {
                kind: "drift".to_string(),
                subject: "hangar".to_string(),
                detail: "Hangar root is not a real directory".to_string(),
                fixed: false,
                repair: DoctorRepair::None,
            });
            return Ok(scan);
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(scan),
        Err(error) => return Err(error),
    }
    let cas_keys = scan_doctor_objects(&hangar, &mut scan.report, write_seals)?;
    scan_doctor_staging(&hangar, &mut scan.report)?;
    scan_doctor_cas(&hangar, &cas_keys, &mut scan.report)?;

    match super::list_read_only_checked(roots) {
        Ok(entries) => doctor_repair_paths(entries, &mut scan.repair_paths),
        Err(error) => scan.report.findings.push(HangarDoctorFinding {
            kind: "drift".to_string(),
            subject: "package records".to_string(),
            detail: format!("could not read: {error}"),
            fixed: false,
            repair: DoctorRepair::None,
        }),
    }
    scan.report
        .findings
        .sort_by(|left, right| doctor_finding_sort_key(left).cmp(&doctor_finding_sort_key(right)));
    Ok(scan)
}

fn doctor_finding_sort_key(finding: &HangarDoctorFinding) -> (u8, &str) {
    let rank = match finding.kind.as_str() {
        "drift" => 0,
        "stale stage" => 1,
        "orphan CAS" => 2,
        _ => 3,
    };
    (rank, finding.subject.as_str())
}

fn scan_doctor_objects(
    hangar: &Path,
    report: &mut HangarDoctorReport,
    write_seals: bool,
) -> std::io::Result<BTreeSet<String>> {
    let objects = hangar.join(OBJECTS_DIR);
    let metadata = match fs::symlink_metadata(&objects) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        report.findings.push(HangarDoctorFinding {
            kind: "drift".to_string(),
            subject: "objects".to_string(),
            detail: "object pool is not a directory".to_string(),
            fixed: false,
            repair: DoctorRepair::None,
        });
        return Ok(BTreeSet::new());
    }

    let mut entries = fs::read_dir(&objects)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    let canonical_hangar = fs::canonicalize(hangar).unwrap_or_else(|_| hangar.to_path_buf());
    let mut cas_keys = BTreeSet::new();
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(PARTIAL_SUFFIX) {
            continue;
        }
        report.objects += 1;
        let metadata = fs::symlink_metadata(&path)?;
        if !valid_receipt_digest(&name) {
            report.findings.push(HangarDoctorFinding {
                kind: "drift".to_string(),
                subject: doctor_subject(hangar, &path),
                detail: "object name is not a content digest".to_string(),
                fixed: false,
                repair: DoctorRepair::None,
            });
            continue;
        }
        if !metadata.file_type().is_symlink() && !metadata.is_dir() && !metadata.is_file() {
            report.findings.push(HangarDoctorFinding {
                kind: "drift".to_string(),
                subject: doctor_subject(hangar, &path),
                detail: "object is not a directory, regular file, or symlink".to_string(),
                fixed: false,
                repair: DoctorRepair::None,
            });
            continue;
        }
        match doctor_object_digest(&path, &canonical_hangar, write_seals) {
            Ok((actual, status)) if actual == name => {
                record_doctor_status(report, status);
            }
            Ok((_actual, status)) => {
                record_doctor_status(report, status);
                report.findings.push(HangarDoctorFinding {
                kind: "drift".to_string(),
                subject: doctor_subject(hangar, &path),
                detail: "content digest differs from the object name".to_string(),
                fixed: false,
                repair: DoctorRepair::Refresh {
                    path: path.clone(),
                    digest: name.clone(),
                },
                })
            }
            Err(error) => report.findings.push(HangarDoctorFinding {
                kind: "drift".to_string(),
                subject: doctor_subject(hangar, &path),
                detail: format!("content digest could not be verified: {error}"),
                fixed: false,
                repair: DoctorRepair::Refresh {
                    path: path.clone(),
                    digest: name.clone(),
                },
            }),
        }
        if metadata.is_dir() {
            collect_doctor_cas_keys(&path, &mut cas_keys);
        }
    }
    Ok(cas_keys)
}

fn collect_doctor_cas_keys(object: &Path, keys: &mut BTreeSet<String>) {
    for file in files_under(object) {
        let Ok(metadata) = fs::symlink_metadata(&file) else {
            continue;
        };
        if !metadata.is_file() || metadata.len() == 0 {
            continue;
        }
        let Ok(bytes) = fs::read(&file) else {
            continue;
        };
        keys.insert(format!(
            "{}-{:08x}",
            SHA256::sha256_hex(&bytes),
            permission_identity(&metadata)
        ));
    }
}

fn scan_doctor_staging(hangar: &Path, report: &mut HangarDoctorReport) -> std::io::Result<()> {
    scan_doctor_stage_dir(hangar, Path::new(STAGE_DIR), false, report)?;
    scan_doctor_stage_dir(hangar, Path::new("stage"), true, report)?;

    let objects = hangar.join(OBJECTS_DIR);
    let Ok(metadata) = fs::symlink_metadata(&objects) else {
        return Ok(());
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Ok(());
    }
    let mut entries = fs::read_dir(objects)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(PARTIAL_SUFFIX) {
            continue;
        }
        let path = entry.path();
        report.findings.push(HangarDoctorFinding {
            kind: "stale stage".to_string(),
            subject: doctor_subject(hangar, &path),
            detail: "abandoned partial object".to_string(),
            fixed: false,
            repair: doctor_remove_repair(&path),
        });
    }
    Ok(())
}

fn scan_doctor_stage_dir(
    hangar: &Path,
    relative: &Path,
    admission_only: bool,
    report: &mut HangarDoctorReport,
) -> std::io::Result<()> {
    let stage = hangar.join(relative);
    let metadata = match fs::symlink_metadata(&stage) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        report.findings.push(HangarDoctorFinding {
            kind: "drift".to_string(),
            subject: doctor_subject(hangar, &stage),
            detail: "staging path is not a directory".to_string(),
            fixed: false,
            repair: DoctorRepair::None,
        });
        return Ok(());
    }
    let mut entries = fs::read_dir(&stage)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if admission_only {
            #[cfg(unix)]
            {
                let name = entry.file_name();
                let is_native_stage = name
                    .to_str()
                    .is_some_and(|value| value.starts_with("nix-cache-"));
                if is_native_stage && !NixCache::admission_stage_is_dead(&name, std::process::id())
                {
                    continue;
                }
            }
        }
        let path = entry.path();
        report.findings.push(HangarDoctorFinding {
            kind: "stale stage".to_string(),
            subject: doctor_subject(hangar, &path),
            detail: "abandoned staging directory".to_string(),
            fixed: false,
            repair: doctor_remove_repair(&path),
        });
    }
    Ok(())
}

fn scan_doctor_cas(
    hangar: &Path,
    referenced: &BTreeSet<String>,
    report: &mut HangarDoctorReport,
) -> std::io::Result<()> {
    let cas = hangar.join(CAS_DIR);
    let metadata = match fs::symlink_metadata(&cas) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        report.findings.push(HangarDoctorFinding {
            kind: "drift".to_string(),
            subject: doctor_subject(hangar, &cas),
            detail: "CAS pool is not a directory".to_string(),
            fixed: false,
            repair: DoctorRepair::None,
        });
        return Ok(());
    }
    let mut entries = fs::read_dir(&cas)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let metadata = fs::symlink_metadata(&path)?;
        if name.ends_with(PARTIAL_SUFFIX) {
            report.findings.push(HangarDoctorFinding {
                kind: "stale stage".to_string(),
                subject: doctor_subject(hangar, &path),
                detail: "abandoned CAS partial".to_string(),
                fixed: false,
                repair: doctor_remove_repair(&path),
            });
            continue;
        }
        if referenced.contains(&name) {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                report.findings.push(HangarDoctorFinding {
                    kind: "drift".to_string(),
                    subject: doctor_subject(hangar, &path),
                    detail: "referenced CAS entry is not a regular file".to_string(),
                    fixed: false,
                    repair: DoctorRepair::None,
                });
            }
            continue;
        }
        report.findings.push(HangarDoctorFinding {
            kind: "orphan CAS".to_string(),
            subject: doctor_subject(hangar, &path),
            detail: "not referenced by any Hangar object".to_string(),
            fixed: false,
            repair: doctor_remove_repair(&path),
        });
    }
    Ok(())
}

fn doctor_repair_paths(entries: Vec<StoreEntry>, paths: &mut BTreeMap<String, String>) {
    for entry in entries {
        let Ok(producer) = ProducerRecord::decode(&entry.producer_record) else {
            continue;
        };
        if producer.provider != "nix" {
            continue;
        }
        let primary = entry.envelope.output_hash;
        if let Some(store_path) = producer.facts.get("nix.store-path") {
            if store_path.starts_with("/nix/store/") {
                paths
                    .entry(primary.clone())
                    .or_insert_with(|| store_path.clone());
            }
        }
        for (key, store_path) in &producer.facts {
            let Some(output_name) = key.strip_prefix("nix.output.") else {
                continue;
            };
            if !store_path.starts_with("/nix/store/") {
                continue;
            }
            let Some(digest) = (output_name == "out")
                .then_some(primary.clone())
                .or_else(|| entry.named_outputs.get(output_name).cloned())
            else {
                continue;
            };
            paths.entry(digest).or_insert_with(|| store_path.clone());
        }
    }
}

fn doctor_object_digest(
    path: &Path,
    hangar: &Path,
    write_seal: bool,
) -> Result<(String, Ingest::VerificationStatus), String> {
    Ingest::verified_output_hash_with_options(
        path,
        Some(hangar),
        false,
        false,
        write_seal,
    )
    .map_err(|error| error.to_string())
}

fn record_doctor_status(report: &mut HangarDoctorReport, status: Ingest::VerificationStatus) {
    if status.reused {
        report.seal_reused += 1;
    }
    if status.resealed {
        report.seal_resealed += 1;
    }
    if status.reseal_needed {
        report.seal_reseal_needed += 1;
    }
}

fn doctor_subject(hangar: &Path, path: &Path) -> String {
    path.strip_prefix(hangar)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn doctor_remove_repair(path: &Path) -> DoctorRepair {
    match fs::symlink_metadata(path) {
        Ok(metadata)
            if !metadata.file_type().is_symlink() && (metadata.is_dir() || metadata.is_file()) =>
        {
            DoctorRepair::Remove(path.to_path_buf())
        }
        _ => DoctorRepair::None,
    }
}

fn remove_doctor_node(path: &Path) -> std::io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "refusing to remove a symlinked Hangar repair path",
        ));
    }
    make_tree_writable_for_removal(path)?;
    if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else if metadata.is_file() {
        fs::remove_file(path)
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Hangar repair path is not removable",
        ))
    }
}

fn hardlink_replace(first: &Path, file: &Path) -> std::io::Result<()> {
    if first == file {
        return Ok(());
    }
    let sequence = OPTIMIZE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let tmp = file.with_extension(format!("jet-dedup-{}-{sequence}", std::process::id()));
    #[cfg(unix)]
    {
        fs::hard_link(first, &tmp)?;
        match fs::rename(&tmp, file) {
            Ok(()) => Ok(()),
            Err(error) => {
                let _ = fs::remove_file(&tmp);
                Err(error)
            }
        }
    }
    #[cfg(not(unix))]
    {
        fs::rename(file, &tmp)?;
        match fs::hard_link(first, file) {
            Ok(()) => {
                let _ = fs::remove_file(&tmp);
                Ok(())
            }
            Err(error) => {
                let _ = fs::rename(&tmp, file);
                Err(error)
            }
        }
    }
}

pub(crate) fn object_dirs(hangar: &Path) -> std::io::Result<Vec<fs::DirEntry>> {
    let mut out = Vec::new();
    for ent in fs::read_dir(hangar)? {
        let ent = ent?;
        let path = ent.path();
        let name = ent.file_name().to_string_lossy().into_owned();
        let reserved = name == BUILD_SCRATCH_DIR
            || name == FAILED_SCRATCH_DIR
            || name == STAGE_DIR
            || name == OBJECTS_DIR
            || name == CAS_DIR
            || name == REFERRERS_DIR
            || name == "receipts"
            || name == SEALS_DIR
            || name == "reproducibility-staging"
            || name == "lifecycle-db"
            || name == "closure-db"
            || name == "quarantine"
            || name.starts_with('.');
        let metadata = fs::symlink_metadata(&path)?;
        if reserved {
            continue;
        }
        if metadata.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Hangar object entry is a symlink: {}", path.display()),
            ));
        }
        if metadata.is_dir() && !reserved {
            out.push(ent);
        }
    }
    out.sort_by_key(|e| e.file_name());
    Ok(out)
}

fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(root) {
        for ent in rd.flatten() {
            let p = ent.path();
            let Ok(metadata) = fs::symlink_metadata(&p) else {
                continue;
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                out.extend(files_under(&p));
            } else if metadata.is_file() {
                out.push(p);
            }
        }
    }
    out
}

pub(crate) fn read_meta(dir: &Path) -> Option<ParsedMeta> {
    let text = fs::read_to_string(dir.join("meta.json")).ok()?;
    parse_meta(&text)
}
