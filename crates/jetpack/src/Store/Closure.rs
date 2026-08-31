use super::Journal::{
    append_entry, apply_entry, canonical_action_projection, closure_graph_structure_read_only,
    compact_if_needed, journal_dir, load_graph, load_graph_structure_mode,
    materialize_package_record, parse_entry, remove_package_record, store_entry_from_meta,
    validate_graph_store_proofs, validate_graph_structure_mode, validate_record_store_proof,
    JournalEntry, JournalKind, PARTIAL_SUFFIX, TXN_SUFFIX,
};
#[cfg(test)]
use super::Journal::{hex, sync_dir, transaction_paths, DB_DIR};
use super::Receipt::{materialize_receipt, prepare_entry_receipt, recover_receipt_staging};
use super::*;

pub(crate) const RECEIPTS_DIR: &str = "receipts";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuEntry {
    pub id: String,
    pub unique_bytes: Option<u64>,
    pub shared_bytes: Option<u64>,
    /// True when the A4 provenance shows a first-party source build.
    pub source_built: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuReport {
    pub objects: usize,
    pub packages: usize,
    pub built: usize,
    pub unique_bytes: Option<u64>,
    pub shared_bytes: Option<u64>,
    pub closure_physical_bytes: Option<u64>,
    pub entries: Vec<DuEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineDuEntry {
    pub root: PathBuf,
    pub hangar: PathBuf,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineDuPool {
    pub path: PathBuf,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineDuReport {
    pub shared_cas: Option<MachineDuPool>,
    pub roots: Vec<MachineDuEntry>,
    pub total_bytes: u64,
}

#[derive(Debug, Default)]
struct PhysicalMeasurement {
    unique_bytes: u64,
    shared_bytes: u64,
    per_object: BTreeMap<String, u64>,
}

/// D-JPK-GC1 / U22: honest root-inclusive closure disk usage. Every live
/// package owns the union of its named-output closures. Physical allocation is
/// counted once per `(device, inode)` and never inferred from logical length.
pub fn du(roots: &Roots) -> std::io::Result<DuReport> {
    super::super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        // Do not migrate here: a damaged metadata path must be compared with
        // the committed journal, not promoted into a new projection.
        let (_, graph) = recover_closure_journal_graph_unlocked(roots)?;
        let entries = super::list_unlocked(roots)?;
        let entry_ids = entries
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<BTreeSet<_>>();
        if let Some(record) = graph
            .records
            .values()
            .find(|record| !entry_ids.contains(record.id.as_str()))
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "closure graph has package record `{}` but its metadata projection is missing",
                    record.id
                ),
            ));
        }
        let mut package_closures = BTreeMap::<String, BTreeSet<String>>::new();
        let mut object_paths = BTreeMap::<String, PathBuf>::new();
        let mut object_owners = BTreeMap::<String, BTreeSet<String>>::new();

        for entry in &entries {
            let record = graph.records.get(&entry.id).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("closure graph has no package record for `{}`", entry.id),
                )
            })?;
            let mut expected_outputs = entry.named_outputs.clone();
            expected_outputs.insert("out".to_string(), entry.envelope.output_hash.clone());
            if record.primary != entry.envelope.output_hash || record.outputs != expected_outputs {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "closure graph outputs for `{}` disagree with its metadata projection",
                        entry.id
                    ),
                ));
            }

            let mut closure = BTreeSet::new();
            for digest in record.outputs.values() {
                closure.extend(graph.closure(digest));
            }
            for digest in &closure {
                let object = graph.objects.get(digest).ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("closure graph has no object `{digest}` for `{}`", entry.id),
                    )
                })?;
                if object.external {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!(
                            "closure object `{digest}` for `{}` is external to Hangar",
                            entry.id
                        ),
                    ));
                }
                let path = PathBuf::from(&object.path);
                if !path.starts_with(roots.hangar_dir()) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("closure object `{digest}` is outside Hangar"),
                    ));
                }
                let metadata = fs::symlink_metadata(&path)?;
                if !metadata.file_type().is_symlink() && !metadata.is_dir() && !metadata.is_file() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("closure object `{digest}` is not a supported filesystem node"),
                    ));
                }
                if let Some(existing) = object_paths.insert(digest.clone(), path.clone()) {
                    if existing != path {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("closure object `{digest}` has conflicting Hangar paths"),
                        ));
                    }
                }
                object_owners
                    .entry(digest.clone())
                    .or_default()
                    .insert(entry.id.clone());
            }
            package_closures.insert(entry.id.clone(), closure);
        }

        let physical = measure_physical(&object_paths, &object_owners)?;
        let built = entries
            .iter()
            .filter(|entry| entry.envelope.provenance.contains("core-"))
            .count();
        let entries = entries
            .into_iter()
            .map(|entry| {
                let (unique_bytes, shared_bytes) = match &physical {
                    Some(measurement) => {
                        let mut unique = 0u64;
                        let mut shared = 0u64;
                        for digest in package_closures.get(&entry.id).into_iter().flatten() {
                            let bytes = measurement.per_object.get(digest).copied().unwrap_or(0);
                            if object_owners
                                .get(digest)
                                .is_some_and(|owners| owners.len() > 1)
                            {
                                shared = shared.checked_add(bytes).ok_or_else(|| {
                                    std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        "Hangar shared disk usage overflowed",
                                    )
                                })?;
                            } else {
                                unique = unique.checked_add(bytes).ok_or_else(|| {
                                    std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        "Hangar unique disk usage overflowed",
                                    )
                                })?;
                            }
                        }
                        (Some(unique), Some(shared))
                    }
                    None => (None, None),
                };
                Ok(DuEntry {
                    id: entry.id,
                    unique_bytes,
                    shared_bytes,
                    source_built: entry.envelope.provenance.contains("core-"),
                })
            })
            .collect::<std::io::Result<Vec<_>>>()?;

        let closure_physical_bytes = physical
            .as_ref()
            .map(|measurement| {
                measurement
                    .unique_bytes
                    .checked_add(measurement.shared_bytes)
                    .ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "Hangar physical disk usage overflowed",
                        )
                    })
            })
            .transpose()?;
        Ok(DuReport {
            objects: object_paths.len(),
            packages: entries.len(),
            built,
            unique_bytes: physical
                .as_ref()
                .map(|measurement| measurement.unique_bytes),
            shared_bytes: physical
                .as_ref()
                .map(|measurement| measurement.shared_bytes),
            closure_physical_bytes,
            entries,
        })
    })
}

/// Report the physical Hangar footprint across every discovered machine root.
/// One inode set spans all roots, so cross-root hardlinks are counted once.
pub fn du_all() -> std::io::Result<MachineDuReport> {
    let mut seen = BTreeSet::new();
    let shared_cas_path = jet_pkg_model::Store::shared_cas_dir();
    let shared_cas = match fs::symlink_metadata(&shared_cas_path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "shared CAS pool is not a real directory: {}",
                        shared_cas_path.display()
                    ),
                ));
            }
            Some(MachineDuPool {
                path: shared_cas_path.clone(),
                bytes: measure_machine_tree(&shared_cas_path, &mut seen)?,
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let mut report = MachineDuReport {
        shared_cas,
        roots: Vec::new(),
        total_bytes: 0,
    };
    if let Some(pool) = &report.shared_cas {
        report.total_bytes = pool.bytes;
    }

    for roots in Roots::machine_roots() {
        let hangar = roots.hangar_dir();
        let metadata = match fs::symlink_metadata(&hangar) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Hangar root is not a real directory: {}", hangar.display()),
            ));
        }
        let bytes = measure_machine_tree(&hangar, &mut seen)?;
        report.total_bytes = report.total_bytes.checked_add(bytes).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "machine Hangar disk usage overflowed",
            )
        })?;
        report.roots.push(MachineDuEntry {
            root: roots.root,
            hangar,
            bytes,
        });
    }
    report
        .roots
        .sort_by(|left, right| left.root.cmp(&right.root));
    Ok(report)
}

#[cfg(unix)]
fn measure_machine_tree(path: &Path, seen: &mut BTreeSet<(u64, u64)>) -> std::io::Result<u64> {
    use std::os::unix::fs::MetadataExt as _;

    let metadata = fs::symlink_metadata(path)?;
    if !seen.insert((metadata.dev(), metadata.ino())) {
        return Ok(0);
    }
    let mut total = metadata.blocks().checked_mul(512).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("physical allocation for `{}` overflowed", path.display()),
        )
    })?;
    if metadata.is_dir() {
        for child in fs::read_dir(path)? {
            total = total
                .checked_add(measure_machine_tree(&child?.path(), seen)?)
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "machine Hangar disk usage overflowed",
                    )
                })?;
        }
    }
    Ok(total)
}

#[cfg(not(unix))]
fn measure_machine_tree(path: &Path, seen: &mut BTreeSet<()>) -> std::io::Result<u64> {
    let metadata = fs::symlink_metadata(path)?;
    let _ = seen;
    let mut total = metadata.len();
    if metadata.is_dir() {
        for child in fs::read_dir(path)? {
            total = total
                .checked_add(measure_machine_tree(&child?.path(), seen)?)
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "machine Hangar disk usage overflowed",
                    )
                })?;
        }
    }
    Ok(total)
}

#[cfg(unix)]
fn measure_physical(
    object_paths: &BTreeMap<String, PathBuf>,
    object_owners: &BTreeMap<String, BTreeSet<String>>,
) -> std::io::Result<Option<PhysicalMeasurement>> {
    use std::os::unix::fs::MetadataExt as _;

    type NodeKey = (u64, u64);
    let mut nodes = BTreeMap::<NodeKey, (u64, BTreeSet<String>)>::new();
    let mut object_nodes = BTreeMap::<String, BTreeSet<NodeKey>>::new();
    let mut expanded = BTreeSet::new();

    fn walk(
        digest: &str,
        path: &Path,
        owners: &BTreeSet<String>,
        nodes: &mut BTreeMap<(u64, u64), (u64, BTreeSet<String>)>,
        object_nodes: &mut BTreeMap<String, BTreeSet<(u64, u64)>>,
        expanded: &mut BTreeSet<(u64, u64)>,
    ) -> std::io::Result<()> {
        let metadata = fs::symlink_metadata(path)?;
        let file_type = metadata.file_type();
        if !file_type.is_symlink() && !metadata.is_dir() && !metadata.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "closure node `{}` is not a supported filesystem node",
                    path.display()
                ),
            ));
        }
        let key = (metadata.dev(), metadata.ino());
        let bytes = metadata.blocks().checked_mul(512).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("physical allocation for `{}` overflowed", path.display()),
            )
        })?;
        let node = nodes.entry(key).or_insert_with(|| (bytes, BTreeSet::new()));
        if node.0 != bytes {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "filesystem allocation changed while measuring `{}`",
                    path.display()
                ),
            ));
        }
        node.1.extend(owners.iter().cloned());
        object_nodes
            .entry(digest.to_string())
            .or_default()
            .insert(key);
        if metadata.is_dir() && expanded.insert(key) {
            for child in fs::read_dir(path)? {
                walk(
                    digest,
                    &child?.path(),
                    owners,
                    nodes,
                    object_nodes,
                    expanded,
                )?;
            }
        }
        Ok(())
    }

    for (digest, path) in object_paths {
        let owners = object_owners.get(digest).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("closure object `{digest}` has no package owner"),
            )
        })?;
        walk(
            digest,
            path,
            owners,
            &mut nodes,
            &mut object_nodes,
            &mut expanded,
        )?;
    }

    let mut measurement = PhysicalMeasurement::default();
    for (digest, node_keys) in object_nodes {
        let bytes = node_keys.iter().try_fold(0u64, |total, key| {
            total
                .checked_add(nodes.get(key).map(|node| node.0).unwrap_or(0))
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("physical allocation for `{digest}` overflowed"),
                    )
                })
        })?;
        measurement.per_object.insert(digest, bytes);
    }
    for (bytes, owners) in nodes.values() {
        let target = if owners.len() > 1 {
            &mut measurement.shared_bytes
        } else {
            &mut measurement.unique_bytes
        };
        *target = target.checked_add(*bytes).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Hangar physical disk usage overflowed",
            )
        })?;
    }
    Ok(Some(measurement))
}

#[cfg(not(unix))]
fn measure_physical(
    _object_paths: &BTreeMap<String, PathBuf>,
    _object_owners: &BTreeMap<String, BTreeSet<String>>,
) -> std::io::Result<Option<PhysicalMeasurement>> {
    Ok(None)
}

/// Total bytes on disk of a local output tree or regular-file root.
pub(crate) fn dir_size(path: &std::path::Path) -> u64 {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return 0;
    };
    if metadata.file_type().is_symlink() {
        return 0;
    }
    if metadata.is_file() {
        return metadata.len();
    }
    if !metadata.is_dir() {
        return 0;
    }
    let mut total = 0u64;
    if let Ok(rd) = fs::read_dir(path) {
        for ent in rd.flatten() {
            let p = ent.path();
            let Ok(metadata) = fs::symlink_metadata(&p) else {
                continue;
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                total += dir_size(&p);
            } else if metadata.is_file() {
                total += metadata.len();
            }
        }
    }
    total
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CleanReport {
    pub removed_objects: usize,
    pub removed_bytes: u64,
    pub quarantined_objects: usize,
    pub removed_receipts: usize,
    pub removed_receipt_bytes: u64,
    pub swept_tmp: usize,
    pub swept_tmp_bytes: u64,
    pub optimized_files: usize,
    pub optimized_bytes: u64,
}

impl CleanReport {
    pub fn is_empty(&self) -> bool {
        self.removed_objects == 0
            && self.removed_bytes == 0
            && self.quarantined_objects == 0
            && self.removed_receipts == 0
            && self.removed_receipt_bytes == 0
            && self.swept_tmp == 0
            && self.swept_tmp_bytes == 0
            && self.optimized_files == 0
            && self.optimized_bytes == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosureObject {
    pub digest: String,
    pub path: String,
    pub external: bool,
}

#[cfg(test)]
mod registration_tests {
    use super::*;

    struct Guard(PathBuf);

    impl Guard {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "jpk-registration-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = make_tree_writable_for_removal(&self.0);
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn roots() -> (Roots, Guard) {
        let guard = Guard::new();
        let roots = Roots {
            root: guard.0.clone(),
            dev_mode: true,
        };
        (roots, guard)
    }

    #[test]
    fn wal_stamp_is_stable_when_directory_insertion_order_changes() {
        let (first_roots, _first_guard) = roots();
        let (second_roots, _second_guard) = roots();
        for (roots, reverse) in [(&first_roots, false), (&second_roots, true)] {
            let journal = journal_dir(roots);
            fs::create_dir_all(&journal).unwrap();
            let names = if reverse { ["z", "a"] } else { ["a", "z"] };
            for name in names {
                let path = journal.join(name);
                fs::write(&path, "same").unwrap();
                fs::File::open(path)
                    .unwrap()
                    .set_times(
                        std::fs::FileTimes::new().set_modified(
                            std::time::UNIX_EPOCH + std::time::Duration::from_secs(1),
                        ),
                    )
                    .unwrap();
            }
        }

        let first = wal_state_stamp(&first_roots).unwrap();
        let second = wal_state_stamp(&second_roots).unwrap();

        assert_eq!(first, second);
    }

    fn identity() -> CacheIdentity {
        CacheIdentity {
            source_fingerprint: "source-v1".into(),
            recipe_fingerprint: "recipe-v1".into(),
            policy_fingerprint: "policy-v1".into(),
            platform: super::super::super::Envelope::host_platform(),
        }
    }

    fn ingest(roots: &Roots, name: &str) -> IngestedObject {
        let source = roots.root.join(format!("fixture-{name}"));
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("payload"), "bytes").unwrap();
        ingest_tree(
            roots,
            &IngestRequest {
                name: name.into(),
                version: "1".into(),
                reference: format!("path:{name}"),
                cache_identity: identity(),
                references: Vec::new(),
                outputs: BTreeMap::from([("out".into(), source)]),
                signature: String::new(),
                provenance: "registration-test".into(),
                platform_artifact_kind: String::new(),
            },
        )
        .unwrap()
    }

    #[cfg(unix)]
    fn ingest_with_reference(roots: &Roots, name: &str, reference: &str) -> IngestedObject {
        let source = roots.root.join(format!("fixture-{name}"));
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("payload"), name.as_bytes()).unwrap();
        ingest_tree(
            roots,
            &IngestRequest {
                name: name.into(),
                version: "1".into(),
                reference: format!("path:{name}"),
                cache_identity: identity(),
                references: vec![reference.to_string()],
                outputs: BTreeMap::from([("out".into(), source)]),
                signature: String::new(),
                provenance: "du-test".into(),
                platform_artifact_kind: String::new(),
            },
        )
        .unwrap()
    }

    #[cfg(unix)]
    fn independent_physical_bytes(paths: impl IntoIterator<Item = PathBuf>) -> u64 {
        use std::os::unix::fs::MetadataExt as _;

        fn walk(path: &Path, seen: &mut BTreeSet<(u64, u64)>) -> u64 {
            let metadata = fs::symlink_metadata(path).unwrap();
            let key = (metadata.dev(), metadata.ino());
            if !seen.insert(key) {
                return 0;
            }
            let mut bytes = metadata.blocks() * 512;
            if metadata.is_dir() {
                for child in fs::read_dir(path).unwrap() {
                    bytes += walk(&child.unwrap().path(), seen);
                }
            }
            bytes
        }

        let mut seen = BTreeSet::new();
        paths.into_iter().map(|path| walk(&path, &mut seen)).sum()
    }

    #[cfg(unix)]
    #[test]
    fn hangar_du_counts_each_closure_object_once_and_splits_shared_bytes() {
        use std::os::unix::fs::symlink;

        let (roots, _guard) = roots();
        let shared = ingest(&roots, "du-shared");
        let first = ingest_with_reference(&roots, "du-first", &shared.entry.envelope.output_hash);
        let second = ingest_with_reference(&roots, "du-second", &shared.entry.envelope.output_hash);

        let first_path = PathBuf::from(&first.entry.out);
        make_tree_writable_for_removal(&first_path).unwrap();
        fs::hard_link(first_path.join("payload"), first_path.join("payload-hard")).unwrap();
        symlink("payload", first_path.join("payload-link")).unwrap();

        let report = du(&roots).unwrap();
        assert_eq!(report.objects, 3);
        assert_eq!(report.packages, 3);
        assert!(report.shared_bytes.unwrap() > 0);
        assert_eq!(
            report.unique_bytes.unwrap() + report.shared_bytes.unwrap(),
            report.closure_physical_bytes.unwrap()
        );
        assert!(report
            .entries
            .iter()
            .find(|entry| entry.id == first.entry.id)
            .is_some_and(|entry| entry.shared_bytes.unwrap() > 0));

        let graph = closure_graph_structure(&roots).unwrap();
        let mut paths = BTreeSet::new();
        for record in graph.records.values() {
            for output in record.outputs.values() {
                for digest in graph.closure(output) {
                    paths.insert(PathBuf::from(&graph.objects[&digest].path));
                }
            }
        }
        assert_eq!(
            report.closure_physical_bytes.unwrap(),
            independent_physical_bytes(paths)
        );
        assert!(second.entry.id != first.entry.id);
    }

    #[cfg(unix)]
    #[test]
    fn hangar_du_never_substitutes_logical_length_for_physical_use() {
        let (roots, _guard) = roots();
        let ingested = ingest(&roots, "du-physical");
        let report = du(&roots).unwrap();
        let logical = fs::read_dir(&ingested.entry.out)
            .unwrap()
            .map(|entry| entry.unwrap().metadata().unwrap().len())
            .sum::<u64>();
        assert!(
            report.closure_physical_bytes.unwrap() > logical,
            "physical allocation must include allocated blocks and directories"
        );
    }

    #[test]
    fn connected_receipt_is_atomic_recoverable_and_corruption_preserves_output() {
        let (roots, _guard) = roots();
        let ingested = ingest(&roots, "receipt-proof");
        assert!(ingested.entry.receipt.starts_with("sha256-"));
        let receipt = roots
            .hangar_dir()
            .join(RECEIPTS_DIR)
            .join(&ingested.entry.receipt);
        let bytes = fs::read(&receipt).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(text.starts_with("jet-development-receipt-v1\n"));
        assert!(text.contains("act\t\t7061636b6167652d7265616c697a6174696f6e\n"));
        assert!(text.contains("closure\t\t7368613235362d"));
        assert!(text.contains("action\t706c616e6e6564\t"));
        assert!(text.contains("activation-proof\t\t\n"));
        assert!(text.contains("witness\t\t"));
        assert!(text.contains("outcome\t\t706173736564\n"));
        let source_digest = format!("sha256-{}", SHA256::sha256_hex("source-v1".as_bytes()));
        let source_input = format!(
            "input\t{}\t{}",
            super::hex("source-fingerprint"),
            super::hex(&source_digest)
        );
        assert!(
            text.contains(&source_input),
            "missing content-addressed input"
        );

        let partial = receipt.with_file_name(".crashed-receipt.partial");
        fs::write(&partial, b"partial receipt").unwrap();
        recover_closure_journal(&roots).unwrap();
        assert!(!partial.exists());

        fs::remove_file(&receipt).unwrap();
        assert!(recover_closure_journal(&roots).unwrap() >= 1);
        assert_eq!(fs::read(&receipt).unwrap(), bytes);

        fs::write(&receipt, b"corrupt receipt").unwrap();
        let error = list_checked(&roots).unwrap_err();
        assert!(error.to_string().contains("receipt"), "{error}");
        assert!(Path::new(&ingested.entry.out).is_dir());
        fs::remove_file(&receipt).unwrap();
        recover_closure_journal(&roots).unwrap();
        assert_eq!(fs::read(&receipt).unwrap(), bytes);
    }

    #[cfg(unix)]
    #[test]
    fn receipt_directory_symlink_is_rejected_without_path_escape() {
        use std::os::unix::fs::symlink;

        let (roots, _guard) = roots();
        let outside = roots.root.join("receipt-outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("keep"), "live").unwrap();
        let receipts = roots.hangar_dir().join(RECEIPTS_DIR);
        fs::create_dir_all(roots.hangar_dir()).unwrap();
        symlink(&outside, &receipts).unwrap();
        let source = roots.root.join("receipt-symlink-source");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("payload"), "bytes").unwrap();
        let result = ingest_tree(
            &roots,
            &IngestRequest {
                name: "receipt-symlink".into(),
                version: "1".into(),
                reference: "path:receipt-symlink".into(),
                cache_identity: identity(),
                references: Vec::new(),
                outputs: BTreeMap::from([("out".into(), source)]),
                signature: String::new(),
                provenance: "registration-test".into(),
                platform_artifact_kind: String::new(),
            },
        );
        assert!(result.is_err());
        assert_eq!(fs::read_to_string(outside.join("keep")).unwrap(), "live");
    }

    #[test]
    fn failed_registration_removes_new_gc_root_from_existing_directory() {
        let (roots, _guard) = roots();
        let dir = roots.hangar_dir().join("existing-entry");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("keep"), "existing metadata").unwrap();
        fs::write(dir.join(NIX_GC_ROOT), "new lease").unwrap();
        rollback_registration_dir(&dir, false, false).unwrap();
        assert!(dir.join("keep").is_file());
        assert!(!dir.join(NIX_GC_ROOT).exists());
    }

    #[test]
    fn external_output_is_rejected_before_registration() {
        let (roots, _guard) = roots();
        let out = roots.root.join("external-output");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("payload"), "stable bytes").unwrap();
        let envelope = super::super::super::Envelope::Envelope::for_output(
            &out.to_string_lossy(),
            "path:external",
            "test",
        );
        let error = super::super::record_verified_mode(
            &roots,
            "external",
            "1",
            "path:external",
            &out.to_string_lossy(),
            "",
            "",
            &envelope,
            &identity(),
            true,
        )
        .unwrap_err();
        assert!(error.to_string().contains("outside Hangar"), "{error}");
        assert!(out.exists());
        assert!(list_checked(&roots).unwrap().is_empty());
        assert!(closure_graph(&roots).unwrap().records.is_empty());
    }

    #[test]
    fn projection_failure_after_journal_commit_is_recoverable_success() {
        let (roots, _guard) = roots();
        let ingested = ingest(&roots, "projection-commit");
        let dir = roots.hangar_dir().join(&ingested.entry.id);
        make_tree_writable_for_removal(&dir).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        fs::remove_dir_all(roots.hangar_dir().join(DB_DIR)).unwrap();
        fs::create_dir_all(&dir).unwrap();
        let blocker = dir.join(format!("meta.json.{}.partial", std::process::id()));
        fs::create_dir(&blocker).unwrap();
        let changed = crate::RuntimePolicy::with_lock(&roots.root, "hangar", || {
            register_entry_unlocked(&roots, &ingested.entry)
        })
        .unwrap();
        assert!(changed);
        assert!(!dir.join("meta.json").exists());
        assert_eq!(transaction_paths(&journal_dir(&roots)).unwrap().len(), 1);
        fs::remove_dir(blocker).unwrap();
        assert_eq!(recover_closure_journal(&roots).unwrap(), 1);
        assert!(dir.join("meta.json").is_file());
    }

    #[test]
    fn disappearing_output_after_journal_append_rolls_back_transaction() {
        let (roots, _guard) = roots();
        let ingested = ingest(&roots, "post-append-race");
        let object = PathBuf::from(&ingested.entry.out);
        let dir = roots.hangar_dir().join(&ingested.entry.id);
        make_tree_writable_for_removal(&dir).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        fs::remove_dir_all(roots.hangar_dir().join(DB_DIR)).unwrap();
        let mut disappeared = false;
        let mut disappear = || {
            make_tree_writable_for_removal(&object).unwrap();
            fs::remove_dir_all(&object).unwrap();
            disappeared = true;
        };
        let error = crate::RuntimePolicy::with_lock(&roots.root, "hangar", || {
            register_entry_unlocked_with_hook(&roots, &ingested.entry, &mut disappear)
        })
        .unwrap_err();
        assert!(disappeared, "race hook must remove the verified output");
        assert!(error.to_string().contains("does not exist"), "{error}");
        assert!(transaction_paths(&journal_dir(&roots)).unwrap().is_empty());
        assert!(list_checked(&roots).unwrap().is_empty());
        assert!(closure_graph(&roots).unwrap().records.is_empty());

        fs::create_dir_all(&object).unwrap();
        fs::write(object.join("payload"), "bytes").unwrap();
        seal_node(&object).unwrap();
        let changed = crate::RuntimePolicy::with_lock(&roots.root, "hangar", || {
            register_entry_unlocked(&roots, &ingested.entry)
        })
        .unwrap();
        assert!(changed);
        assert_eq!(list_checked(&roots).unwrap(), vec![ingested.entry]);
    }

    #[test]
    fn batch_registration_hashes_each_object_once() {
        let (roots, _guard) = roots();
        let mut entries = (0..3)
            .map(|index| ingest(&roots, &format!("batch-{index}")))
            .map(|ingested| ingested.entry)
            .collect::<Vec<_>>();

        for entry in &mut entries {
            assert!(remove_closure_record(&roots, &entry.id).unwrap());
            let output = Path::new(&entry.out);
            super::super::Ingest::invalidate_verified_digest(output);
            super::super::Ingest::reset_verified_digest_hash_count(output);

            let dependency = output.join("payload");
            let digest = format!("sha256-{}", SHA256::sha256_file_hex(&dependency).unwrap());
            let mut producer = ProducerRecord::decode(&entry.producer_record).unwrap();
            producer
                .facts
                .insert(format!("dependency.object.{digest}"), "payload".into());
            entry.producer_record = producer.encode();
            entry.references.push(digest);
            super::super::Ingest::invalidate_verified_digest(&dependency);
            super::super::Ingest::reset_verified_digest_hash_count(&dependency);
        }

        assert!(crate::RuntimePolicy::with_lock(&roots.root, "hangar", || {
            register_entries_unlocked(&roots, &entries)
        })
        .unwrap());

        for entry in &entries {
            let output = Path::new(&entry.out);
            // Sealed verification manifests allow zero content hashes when a
            // valid seal is trusted; the batch contract is at most one.
            assert!(
                super::super::Ingest::verified_digest_hash_count(output) <= 1,
                "output `{}` was hashed more than once",
                entry.out
            );
            assert!(
                super::super::Ingest::verified_digest_hash_count(&output.join("payload")) <= 1,
                "dependency object for `{}` was hashed more than once",
                entry.out
            );
        }
    }

    #[test]
    fn admitted_nix_batch_verifies_each_shared_output_once() {
        let (roots, _guard) = roots();
        let mut entries = (0..3)
            .map(|index| ingest(&roots, &format!("admitted-nix-{index}")))
            .map(|ingested| ingested.entry)
            .collect::<Vec<_>>();

        for (index, entry) in entries.iter_mut().enumerate() {
            assert!(remove_closure_record(&roots, &entry.id).unwrap());
            let output = Path::new(&entry.out);
            super::super::Ingest::invalidate_verified_digest(output);
            super::super::Ingest::reset_verified_digest_hash_count(output);

            let dependency = output.join("payload");
            let digest = format!("sha256-{}", SHA256::sha256_file_hex(&dependency).unwrap());
            let mut producer = ProducerRecord::decode(&entry.producer_record).unwrap();
            producer.provider = "nix".into();
            producer.immutable_source = format!("nix-source-{index}");
            producer
                .facts
                .insert("nix.output.out".into(), entry.out.clone());
            producer
                .facts
                .insert(format!("dependency.object.{digest}"), "payload".into());
            entry.producer_record = producer.encode();
            entry.references.push(digest);
            super::super::Ingest::invalidate_verified_digest(&dependency);
            super::super::Ingest::reset_verified_digest_hash_count(&dependency);
        }

        assert!(crate::RuntimePolicy::with_lock(&roots.root, "hangar", || {
            register_admitted_nix_entries_unlocked(&roots, &entries)
        })
        .unwrap());

        for entry in &entries {
            let output = Path::new(&entry.out);
            // Sealed verification manifests allow zero content hashes when a
            // valid seal is trusted; the batch contract is at most one.
            assert!(
                super::super::Ingest::verified_digest_hash_count(output) <= 1,
                "admitted output `{}` was hashed more than once",
                entry.out
            );
            assert!(
                super::super::Ingest::verified_digest_hash_count(&output.join("payload")) <= 1,
                "admitted dependency object for `{}` was hashed more than once",
                entry.out
            );
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosureRecord {
    pub id: String,
    pub primary: String,
    pub action_key: String,
    pub outputs: BTreeMap<String, String>,
    pub references: BTreeSet<String>,
    /// Canonical versioned producer replay record.
    pub producer_record: String,
    /// Exact package metadata projection recoverable from committed WAL.
    pub package_meta: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClosureGraph {
    pub objects: BTreeMap<String, ClosureObject>,
    pub records: BTreeMap<String, ClosureRecord>,
    pub deleted_records: BTreeSet<String>,
}

/// Validate that a provider's closure edges stay inside its artifact universe.
/// `nix` is the compatibility universe; every other provider is native. The
/// path check catches a native edge to an external compatibility object, while
/// the owner check catches a compat-root object that happens to live under
/// Hangar.
pub(super) fn validate_universe_references<'a>(
    provider: &str,
    references: impl IntoIterator<Item = &'a String>,
    graph: &ClosureGraph,
) -> Result<(), String> {
    let provider_is_compat = provider == "nix";
    for digest in references {
        let Some(object) = graph.objects.get(digest) else {
            // The normal graph validator reports missing objects. Keep this
            // helper focused on the universe boundary for pre-registration
            // checks, where the candidate has not entered the graph yet.
            continue;
        };
        if !provider_is_compat && object.external {
            return Err(format!(
                "native provider `{provider}` crosses the ABI universe via external compatibility object `{digest}` at `{}`",
                object.path
            ));
        }
        for owner in graph
            .records
            .values()
            .filter(|record| record.outputs.values().any(|output| output == digest))
        {
            let owner_provider = match ProducerRecord::decode(&owner.producer_record) {
                Ok(producer) => producer.provider,
                Err(_) if owner.producer_record.is_empty() => continue,
                Err(error) => {
                    return Err(format!(
                        "closure record `{}` has invalid producer record: {error}",
                        owner.id
                    ));
                }
            };
            if (owner_provider == "nix") != provider_is_compat {
                return Err(format!(
                    "provider `{provider}` crosses the ABI universe at object `{digest}` owned by `{owner_provider}`"
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn validate_universe_isolation(
    graph: &ClosureGraph,
    allow_legacy: bool,
) -> Result<(), String> {
    for record in graph.records.values() {
        if allow_legacy && record.producer_record.is_empty() {
            continue;
        }
        let provider = ProducerRecord::decode(&record.producer_record)
            .map_err(|error| {
                format!(
                    "closure record `{}` has invalid producer record: {error}",
                    record.id
                )
            })?
            .provider;
        validate_universe_references(&provider, &record.references, graph)?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CanonicalActionRecord {
    pub(super) outputs: BTreeMap<String, String>,
    pub(super) references: BTreeSet<String>,
}

impl ClosureGraph {
    pub fn direct_references(&self, digest: &str) -> Vec<String> {
        self.reference_index()
            .remove(digest)
            .unwrap_or_default()
            .into_iter()
            .map(str::to_string)
            .collect()
    }

    pub fn transitive_references(&self, digest: &str) -> Vec<String> {
        let index = self.reference_index();
        let mut seen = BTreeSet::new();
        let mut pending = index
            .get(digest)
            .into_iter()
            .flat_map(|references| references.iter().copied())
            .collect::<Vec<_>>();
        while let Some(next) = pending.pop() {
            if next == digest || !seen.insert(next) {
                continue;
            }
            if let Some(references) = index.get(next) {
                pending.extend(references.iter().copied());
            }
        }
        seen.into_iter().map(str::to_string).collect()
    }

    pub fn closure(&self, digest: &str) -> Vec<String> {
        let mut closure = self.transitive_references(digest);
        closure.push(digest.to_string());
        closure.sort();
        closure.dedup();
        closure
    }

    pub fn referrers(&self, digest: &str) -> Vec<String> {
        self.referrer_index()
            .remove(digest)
            .unwrap_or_default()
            .into_iter()
            .map(str::to_string)
            .collect()
    }

    pub fn transitive_referrers(&self, digest: &str) -> Vec<String> {
        let index = self.referrer_index();
        let mut seen = BTreeSet::new();
        let mut pending = index
            .get(digest)
            .into_iter()
            .flat_map(|referrers| referrers.iter().copied())
            .collect::<Vec<_>>();
        while let Some(next) = pending.pop() {
            if next == digest || !seen.insert(next) {
                continue;
            }
            if let Some(referrers) = index.get(next) {
                pending.extend(referrers.iter().copied());
            }
        }
        seen.into_iter().map(str::to_string).collect()
    }

    pub fn reverse_closure(&self, digest: &str) -> Vec<String> {
        let mut closure = self.transitive_referrers(digest);
        closure.push(digest.to_string());
        closure.sort();
        closure.dedup();
        closure
    }

    pub fn action_outputs(&self, action_key: &str) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        for record in self
            .records
            .values()
            .filter(|record| record.action_key == action_key)
        {
            if let Ok(projection) = canonical_action_projection(record) {
                for (name, digest) in projection.outputs {
                    out.insert(name, digest);
                }
            }
        }
        out
    }

    pub fn actions_for_output(&self, digest: &str) -> Vec<String> {
        self.records
            .values()
            .filter(|record| record.outputs.values().any(|output| output == digest))
            .map(|record| record.action_key.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    fn reference_index(&self) -> BTreeMap<&str, BTreeSet<&str>> {
        let mut index = BTreeMap::new();
        for record in self.records.values() {
            for output in record.outputs.values() {
                index
                    .entry(output.as_str())
                    .or_insert_with(BTreeSet::new)
                    .extend(record.references.iter().map(String::as_str));
            }
        }
        index
    }

    fn referrer_index(&self) -> BTreeMap<&str, BTreeSet<&str>> {
        let mut index = BTreeMap::new();
        for record in self.records.values() {
            for reference in &record.references {
                index
                    .entry(reference.as_str())
                    .or_insert_with(BTreeSet::new)
                    .extend(record.outputs.values().map(String::as_str));
            }
        }
        index
    }
}

pub fn closure_graph(roots: &Roots) -> std::io::Result<ClosureGraph> {
    super::super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        let (_, graph) = migrate_closure_graph_unlocked(roots)?;
        validate_graph_store_proofs(roots, &graph, false).map_err(std::io::Error::other)?;
        Ok(graph)
    })
}

pub fn closure_graph_structure(roots: &Roots) -> std::io::Result<ClosureGraph> {
    super::super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        closure_graph_structure_unlocked(roots)
    })
}

static GRAPH_CACHE: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, (String, ClosureGraph)>>,
> = std::sync::LazyLock::new(Default::default);

/// One command rebuilt this graph once per package (28 times for the repo's own
/// env), and the first build dominates a warm entry. The migration + graph read
/// is a pure function of committed journal state, so memoize it on the WAL
/// state stamp: any commit changes the stamp and invalidates the entry, so a
/// concurrent writer is never served a stale graph.
pub(super) fn closure_graph_structure_unlocked(roots: &Roots) -> std::io::Result<ClosureGraph> {
    let stamp = wal_state_stamp(roots)?;
    if let Ok(cache) = GRAPH_CACHE.lock() {
        if let Some((cached_stamp, graph)) = cache.get(&roots.root) {
            if *cached_stamp == stamp {
                return Ok(graph.clone());
            }
        }
    }
    let graph = migrate_closure_graph_unlocked(roots).map(|(_, graph)| graph)?;
    if let Ok(mut cache) = GRAPH_CACHE.lock() {
        cache.insert(roots.root.clone(), (stamp, graph.clone()));
    }
    Ok(graph)
}

pub(super) fn lifecycle_closure_graph_unlocked(roots: &Roots) -> std::io::Result<ClosureGraph> {
    let (_, graph) = migrate_closure_graph_unlocked(roots)?;
    validate_graph_store_proofs(roots, &graph, false).map_err(std::io::Error::other)?;
    Ok(graph)
}

/// GC plan variant: skip package projections already identified as malformed.
/// The plan must inspect valid legacy records without letting one bad projection
/// block the whole store, but it must not repair the bad projection before the
/// plan can name it for quarantine.
pub(super) fn lifecycle_closure_graph_unlocked_ignoring(
    roots: &Roots,
    ignored: &BTreeSet<String>,
) -> std::io::Result<ClosureGraph> {
    let (_, graph) = migrate_closure_graph_unlocked_ignoring(roots, ignored)?;
    validate_graph_store_proofs(roots, &graph, false).map_err(std::io::Error::other)?;
    Ok(graph)
}

pub(super) fn entry_closure_store_proof(
    roots: &Roots,
    graph: &ClosureGraph,
    entry: &StoreEntry,
) -> bool {
    let Some(record) = graph.records.get(&entry.id) else {
        return false;
    };
    let mut outputs = entry.named_outputs.clone();
    outputs.insert("out".to_string(), entry.envelope.output_hash.clone());
    if record.primary != entry.envelope.output_hash
        || record.action_key != entry_action_key(entry)
        || record.outputs != outputs
        || record.references != entry.references.iter().cloned().collect()
    {
        return false;
    }
    graph
        .transitive_references(&record.primary)
        .into_iter()
        .all(|digest| closure_object_rehashes(roots, graph, &digest))
}

/// Closure members already proven against their seals in this process. A
/// 28-package env shares one toolchain closure; without this each package
/// re-stat-walks every shared member (~28× the whole hangar). Entry-level
/// verification (`try_entry_output_hash`) never consults this memo, so the
/// sealed drift law for package outputs is unchanged; member drift is caught
/// by the next command — the same trust window D-JPK-VERIFYONCE1=A ratified
/// for stat-identical content. The set is keyed by (hangar root, digest), so
/// a member replaced under a different digest never hits.
static PROVEN_MEMBERS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashSet<(PathBuf, String)>>,
> = std::sync::LazyLock::new(Default::default);

pub(super) fn invalidate_proven_member(hangar: &Path, digest: &str) {
    if let Ok(mut proven) = PROVEN_MEMBERS.lock() {
        proven.remove(&(hangar.to_path_buf(), digest.to_string()));
    }
}

fn closure_object_rehashes(roots: &Roots, graph: &ClosureGraph, digest: &str) -> bool {
    let key = (roots.hangar_dir(), digest.to_string());
    if let Ok(proven) = PROVEN_MEMBERS.lock() {
        if proven.contains(&key) {
            return true;
        }
    }
    let ok = closure_object_rehashes_uncached(roots, graph, digest);
    if ok {
        if let Ok(mut proven) = PROVEN_MEMBERS.lock() {
            proven.insert(key);
        }
    }
    ok
}

fn closure_object_rehashes_uncached(roots: &Roots, graph: &ClosureGraph, digest: &str) -> bool {
    let Some(object) = graph.objects.get(digest) else {
        return false;
    };
    let owners = graph
        .records
        .values()
        .filter(|record| record.outputs.values().any(|output| output == digest));
    let mut found_owner = false;
    for record in owners {
        found_owner = true;
        let Some(meta) = parse_meta(&record.package_meta) else {
            return false;
        };
        // Hash the owned object itself, not the owner's primary output: a
        // multi-output record (`bashInteractive` man/doc, `util-linux` bin)
        // owns members whose digests are not its primary hash.
        let owner_entry = store_entry_from_meta(&record.id, &meta);
        if !Ingest::verified_output_hash_persistent(
            Path::new(&object.path),
            Some(&roots.hangar_dir()),
            !owner_entry.platform_artifact_kind.is_empty(),
        )
        .is_ok_and(|actual| actual == digest)
        {
            return false;
        }
    }
    if found_owner {
        return true;
    }
    // Every admitted object — directory, regular-file, or symlink root — is
    // named by its canonical output hash and sealed at admission. The old
    // plain-bytes fast path predates file-root admission and returned a
    // different digest for file roots, poisoning every closure that contains
    // one (`jetpack env` re-substituted such packages on every run).
    Ingest::verified_output_hash_persistent(
        Path::new(&object.path),
        Some(&roots.hangar_dir()),
        false,
    )
    .is_ok_and(|actual| actual == digest)
}

pub fn direct_references_of(roots: &Roots, digest: &str) -> std::io::Result<Vec<String>> {
    Ok(closure_graph(roots)?.direct_references(digest))
}

pub fn transitive_references_of(roots: &Roots, digest: &str) -> std::io::Result<Vec<String>> {
    Ok(closure_graph(roots)?.transitive_references(digest))
}

pub fn closure_of(roots: &Roots, digest: &str) -> std::io::Result<Vec<String>> {
    Ok(closure_graph(roots)?.closure(digest))
}

pub fn referrers_of(roots: &Roots, digest: &str) -> std::io::Result<Vec<String>> {
    Ok(closure_graph(roots)?.referrers(digest))
}

pub fn transitive_referrers_of(roots: &Roots, digest: &str) -> std::io::Result<Vec<String>> {
    Ok(closure_graph(roots)?.transitive_referrers(digest))
}

pub fn reverse_closure_of(roots: &Roots, digest: &str) -> std::io::Result<Vec<String>> {
    Ok(closure_graph(roots)?.reverse_closure(digest))
}

pub fn action_outputs_of(
    roots: &Roots,
    action_key: &str,
) -> std::io::Result<BTreeMap<String, String>> {
    Ok(closure_graph(roots)?.action_outputs(action_key))
}

pub fn actions_for_output(roots: &Roots, digest: &str) -> std::io::Result<Vec<String>> {
    Ok(closure_graph(roots)?.actions_for_output(digest))
}

pub fn entry_action_key(entry: &StoreEntry) -> String {
    let identity = &entry.cache_identity;
    let producer = ProducerRecord::decode(&entry.producer_record);
    let nix_producer = producer
        .as_ref()
        .ok()
        .filter(|record| record.provider == "nix");
    let nix = nix_producer.is_some();
    let mut canonical = b"jet.action-store.v5\0".to_vec();
    if let Some(producer) = nix_producer {
        // A Nix derivation path is the canonical input/action identity. Output
        // paths and output-derived cache fingerprints are consequences; using
        // them here would let the same action silently map to different bytes.
        push_frame(&mut canonical, producer.immutable_source.as_bytes());
        for field in [
            identity.recipe_fingerprint.as_bytes(),
            identity.policy_fingerprint.as_bytes(),
            identity.platform.as_bytes(),
        ] {
            push_frame(&mut canonical, field);
        }
    } else {
        for field in [
            entry.reference.as_bytes(),
            identity.source_fingerprint.as_bytes(),
            identity.recipe_fingerprint.as_bytes(),
            identity.policy_fingerprint.as_bytes(),
            identity.platform.as_bytes(),
        ] {
            push_frame(&mut canonical, field);
        }
    }
    if let Ok(producer) = producer {
        for field in [
            producer.provider.as_bytes(),
            producer.toolchain_facts.as_bytes(),
            producer.policy_facts.as_bytes(),
        ] {
            push_frame(&mut canonical, field);
        }
        for (key, value) in
            producer.plan.facts().iter().filter(|(key, _)| {
                action_replay_fact(key) && !(nix && key.as_str() == "nix.reference")
            })
        {
            push_frame(&mut canonical, key.as_bytes());
            push_frame(&mut canonical, value.as_bytes());
        }
    } else {
        // Validation rejects malformed producer records. Keep the key stable
        // until that fail-closed boundary instead of panicking here.
        push_frame(&mut canonical, b"invalid-producer-record");
    }
    let references = entry.references.iter().collect::<BTreeSet<_>>();
    canonical.extend_from_slice(&(references.len() as u64).to_be_bytes());
    for digest in references {
        push_frame(&mut canonical, digest.as_bytes());
    }
    format!("sha256-{}", SHA256::sha256_hex(&canonical))
}

fn action_replay_fact(key: &str) -> bool {
    !key.starts_with("nix.output.") && !key.starts_with("output.") && key != "cache.reproducibility"
}

fn push_frame(out: &mut Vec<u8>, field: &[u8]) {
    out.extend_from_slice(&(field.len() as u64).to_be_bytes());
    out.extend_from_slice(field);
}

pub fn migrate_closure_graph(roots: &Roots) -> std::io::Result<usize> {
    super::super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        migrate_closure_graph_unlocked(roots).map(|(migrated, _)| migrated)
    })
}

/// Process-local cache of the structure-validated closure graph.
///
/// One `jetpack env` run loads the graph once per package per verification
/// pass; each uncached load re-parses every journal transaction and
/// re-verifies every signed receipt (measured: ~23 CPU-minutes for a
/// 28-package env). The cache key is the complete on-disk WAL identity —
/// every journal, receipt, and entry-meta (name, size, mtime) tuple — so a
/// mutation from THIS process or any concurrent jetpack instance changes the
/// stamp and forces a fresh load. Callers hold the hangar lock, so a stamp
/// computed here cannot race a writer.
static STRUCTURE_GRAPH_CACHE: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<PathBuf, (String, ClosureGraph)>>,
> = std::sync::LazyLock::new(Default::default);

fn stamp_mtime_ns(metadata: &fs::Metadata) -> i128 {
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

fn push_stat(table: &mut Vec<u8>, name: &std::ffi::OsStr, metadata: &fs::Metadata) {
    let name = name.as_encoded_bytes();
    table.extend_from_slice(&(name.len() as u64).to_le_bytes());
    table.extend_from_slice(name);
    table.extend_from_slice(&metadata.len().to_le_bytes());
    table.extend_from_slice(&stamp_mtime_ns(metadata).to_le_bytes());
    let kind = if metadata.file_type().is_symlink() {
        b"symlink".as_slice()
    } else if metadata.is_dir() {
        b"directory".as_slice()
    } else if metadata.is_file() {
        b"file".as_slice()
    } else {
        b"other".as_slice()
    };
    table.extend_from_slice(&(kind.len() as u64).to_le_bytes());
    table.extend_from_slice(kind);
}

fn push_dir_stats(table: &mut Vec<u8>, dir: &Path) -> std::io::Result<()> {
    let directory = match fs::symlink_metadata(dir) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            table.extend_from_slice(b"directory-missing\0");
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    table.extend_from_slice(b"directory-present\0");
    if !directory.is_dir() {
        push_stat(table, dir.as_os_str(), &directory);
        return Ok(());
    }
    let mut entries = fs::read_dir(dir)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by(|left, right| {
        left.file_name()
            .as_encoded_bytes()
            .cmp(right.file_name().as_encoded_bytes())
    });
    for entry in entries {
        let metadata = fs::symlink_metadata(entry.path())?;
        push_stat(table, &entry.file_name(), &metadata);
    }
    Ok(())
}

pub(super) fn wal_state_stamp(roots: &Roots) -> std::io::Result<String> {
    let mut table = Vec::new();
    push_dir_stats(&mut table, &journal_dir(roots))?;
    push_dir_stats(&mut table, &roots.hangar_dir().join(RECEIPTS_DIR))?;
    push_dir_stats(&mut table, &roots.hangar_dir().join(super::SEALS_DIR))?;
    push_dir_stats(&mut table, &roots.hangar_dir().join(super::OBJECTS_DIR))?;
    // Entry set: a new or deleted entry must invalidate, but the entry NAME
    // alone is enough — meta content is authenticated against its immutable
    // receipt on every load, and warm runs bump `last_used_at` (meta mtime)
    // on every cached use, which would otherwise evict the cache mid-run.
    let hangar = roots.hangar_dir();
    match fs::read_dir(&hangar) {
        Ok(entries) => {
            let mut names = Vec::new();
            for entry in entries {
                let entry = entry?;
                if fs::symlink_metadata(entry.path().join("meta.json")).is_ok() {
                    names.push(entry.file_name());
                }
            }
            names.sort_by(|left, right| left.as_encoded_bytes().cmp(right.as_encoded_bytes()));
            for name in names {
                let name = name.as_encoded_bytes();
                table.extend_from_slice(&(name.len() as u64).to_le_bytes());
                table.extend_from_slice(name);
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(super::super::SHA256::sha256_hex(&table))
}

pub(super) fn migrate_closure_graph_unlocked(
    roots: &Roots,
) -> std::io::Result<(usize, ClosureGraph)> {
    let stamp = wal_state_stamp(roots)?;
    let cache = &*STRUCTURE_GRAPH_CACHE;
    if let Ok(cache) = cache.lock() {
        if let Some((cached_stamp, graph)) = cache.get(&roots.root) {
            if *cached_stamp == stamp {
                // The migration already ran and persisted; a cached load does
                // no new work, so it reports zero migrated projections.
                return Ok((0, graph.clone()));
            }
        }
    }
    let (_, graph) = recover_closure_journal_graph_unlocked(roots)?;
    let mut entries = list_unlocked(roots)?;
    entries.sort_by(|left, right| left.id.cmp(&right.id));
    let (migrated, graph) = migrate_closure_graph_from_entries(roots, graph, entries)?;
    // Recovery or migration may have rewritten WAL state; stamp the result so
    // the next caller hits.
    let stamp = wal_state_stamp(roots)?;
    if let Ok(mut cache) = cache.lock() {
        cache.insert(roots.root.clone(), (stamp, graph.clone()));
    }
    Ok((migrated, graph))
}

fn migrate_closure_graph_unlocked_ignoring(
    roots: &Roots,
    ignored: &BTreeSet<String>,
) -> std::io::Result<(usize, ClosureGraph)> {
    let mut graph = closure_graph_structure_read_only(roots)?;
    for id in ignored {
        graph.records.remove(id);
        graph.deleted_records.insert(id.clone());
    }
    let mut entries = super::list_read_only(roots)
        .into_iter()
        .filter(|entry| !ignored.contains(&entry.id))
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.id.cmp(&right.id));
    migrate_closure_graph_from_entries(roots, graph, entries)
}

fn migrate_closure_graph_from_entries(
    roots: &Roots,
    mut graph: ClosureGraph,
    entries: Vec<StoreEntry>,
) -> std::io::Result<(usize, ClosureGraph)> {
    let mut seen_records = BTreeSet::new();
    let mut objects = BTreeMap::new();
    let mut records = Vec::new();
    for entry in entries {
        let entry = normalize_legacy_entry(entry)?;
        if graph.deleted_records.contains(&entry.id) {
            continue;
        }
        let (descriptors, record) = descriptor_for_entry(roots, &entry)?;
        if !seen_records.insert(record.id.clone()) {
            return Err(std::io::Error::other(format!(
                "legacy migration contains duplicate record `{}`",
                record.id
            )));
        }
        for object in descriptors {
            if let Some(existing) = graph.objects.get(&object.digest) {
                if existing != &object {
                    return Err(std::io::Error::other(format!(
                        "immutable closure object `{}` changed descriptor",
                        object.digest
                    )));
                }
            } else if let Some(existing) = objects.insert(object.digest.clone(), object.clone()) {
                if existing != object {
                    return Err(std::io::Error::other(format!(
                        "legacy migration gives object `{}` conflicting descriptors",
                        object.digest
                    )));
                }
            }
        }
        if graph.records.get(&record.id) != Some(&record) {
            records.push(record);
        }
    }
    if objects.is_empty() && records.is_empty() {
        // Steady state is exactly where an overgrown journal must fold into
        // one snapshot; skipping here meant a warm store never compacted.
        compact_if_needed(roots)?;
        return Ok((0, graph));
    }
    let migrated = records.len();
    let transaction = JournalEntry {
        kind: JournalKind::Delta,
        objects: objects.into_values().collect(),
        records,
        deleted_records: Vec::new(),
    };
    apply_entry(&mut graph, transaction.clone()).map_err(std::io::Error::other)?;
    validate_graph_structure_mode(roots, &graph, false).map_err(std::io::Error::other)?;
    for record in &transaction.records {
        validate_record_store_proof(roots, record, false).map_err(std::io::Error::other)?;
    }
    append_entry(roots, &transaction)?;
    for record in &transaction.records {
        materialize_package_record(roots, record)?;
    }
    compact_if_needed(roots)?;
    Ok((migrated, graph))
}

/// Single-entry registration seam. Production registration goes through the
/// batch transaction; this remains for tests and the `test-seam` backdater,
/// which deliberately re-runs the full validation path for one entry.
#[cfg(any(test, feature = "test-seam"))]
pub(crate) fn register_entry_unlocked(roots: &Roots, entry: &StoreEntry) -> std::io::Result<bool> {
    register_entries_unlocked_with_mode(
        roots,
        std::slice::from_ref(entry),
        RegistrationMode::Native,
        None,
    )
}

/// Register a batch of already-quarantined entries as one closure transaction.
/// The caller owns the surrounding Hangar lock and must have made every output
/// path live before calling this function. All graph conflicts and store proofs
/// are checked before the single durable journal append, so a failed batch
/// cannot leave an earlier entry committed.
pub(crate) fn register_entries_unlocked(
    roots: &Roots,
    entries: &[StoreEntry],
) -> std::io::Result<bool> {
    register_entries_unlocked_with_mode(roots, entries, RegistrationMode::Native, None)
}

#[cfg(test)]
pub(crate) fn register_admitted_nix_entries_unlocked(
    roots: &Roots,
    entries: &[StoreEntry],
) -> std::io::Result<bool> {
    register_entries_unlocked_with_mode(roots, entries, RegistrationMode::AdmittedNix, None)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegistrationMode {
    Native,
    AdmittedNix,
}

/// Shared closure-registration half of the Hangar admission transaction.
/// Format adapters choose only the output-proof law; certification, conflict
/// checks, journal append, and recoverable projection stay one mechanism.
pub(crate) fn register_entries_unlocked_with_mode(
    roots: &Roots,
    entries: &[StoreEntry],
    mode: RegistrationMode,
    fresh_action_key: Option<&str>,
) -> std::io::Result<bool> {
    if entries.is_empty() {
        return Ok(false);
    }
    for entry in entries {
        if matches!(mode, RegistrationMode::Native) {
            Ingest::share_tree_files(
                roots,
                Path::new(&entry.out),
                !entry.platform_artifact_kind.is_empty(),
            )?;
        } else {
            // Nix admission keeps objects root-local. Optional shared-CAS
            // pooling is skipped, including for regular-file roots.
            verify_admitted_nix_output(roots, entry)?;
        }
        // Native registration may replace payload files with shared-CAS
        // hardlinks. Refresh only the stat manifest: the transaction already
        // checked the canonical digest before publication.
        if !entry.envelope.output_hash.is_empty() {
            let digest = entry.envelope.output_hash.as_str();
            if object_digest_for_path(Path::new(&entry.out), &roots.hangar_dir()).as_deref()
                == Some(digest)
            {
                write_seal(Path::new(&entry.out), &roots.hangar_dir(), digest)?;
            }
        }
        verify_registration_output(roots, entry)?;
    }
    let (_, graph) = migrate_closure_graph_unlocked(roots)?;
    if let Some(action_key) = fresh_action_key {
        for entry in entries {
            super::Reproducibility::certify_registration_unlocked_with_fresh_agreement(
                roots, entry, entries, action_key,
            )?;
        }
    } else {
        super::Reproducibility::certify_registrations_unlocked(roots, entries)?;
    }
    let mut object_map = BTreeMap::new();
    let mut records = Vec::new();
    let mut seen_records = BTreeSet::new();
    for entry in entries {
        let (objects, record) = descriptor_for_entry(roots, entry)?;
        if !seen_records.insert(record.id.clone()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("closure batch contains duplicate record `{}`", record.id),
            ));
        }
        for object in objects {
            if let Some(existing) = graph.objects.get(&object.digest) {
                if existing != &object {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!(
                            "immutable closure object `{}` changed descriptor",
                            object.digest
                        ),
                    ));
                }
            } else if let Some(existing) = object_map.insert(object.digest.clone(), object.clone())
            {
                if existing != object {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!(
                            "closure batch gives object `{}` conflicting descriptors",
                            object.digest
                        ),
                    ));
                }
            }
        }
        if graph.records.get(&record.id) != Some(&record) {
            records.push(record);
        }
    }
    if records.is_empty() && object_map.is_empty() {
        return Ok(false);
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    let transaction = JournalEntry {
        kind: JournalKind::Delta,
        objects: object_map.into_values().collect(),
        records,
        deleted_records: Vec::new(),
    };
    let mut candidate = graph;
    apply_entry(&mut candidate, transaction.clone()).map_err(std::io::Error::other)?;
    validate_graph_structure_mode(roots, &candidate, false).map_err(std::io::Error::other)?;
    for record in &transaction.records {
        validate_record_store_proof(roots, record, false).map_err(std::io::Error::other)?;
    }
    append_entry(roots, &transaction)?;
    for record in &transaction.records {
        let _ = materialize_package_record(roots, record);
    }
    let _ = compact_if_needed(roots);
    Ok(true)
}

fn verify_admitted_nix_output(roots: &Roots, entry: &StoreEntry) -> std::io::Result<()> {
    let producer = ProducerRecord::decode(&entry.producer_record).map_err(std::io::Error::other)?;
    let expected = roots
        .hangar_dir()
        .join(OBJECTS_DIR)
        .join(&entry.envelope.output_hash);
    let metadata = fs::symlink_metadata(&expected).map_err(|error| {
        std::io::Error::other(format!(
            "admitted Nix entry `{}` is not its verified canonical CAS object: canonical CAS path `{}` could not be inspected: {error}",
            entry.out,
            expected.display(),
        ))
    })?;
    let reason = if producer.provider != "nix" {
        Some("producer provider is not `nix`")
    } else if Path::new(&entry.out) != expected {
        Some("entry output path does not equal its output-hash CAS path")
    } else if entry.envelope.output_hash.is_empty() {
        Some("output hash is empty")
    } else if !metadata.file_type().is_symlink() && !metadata.is_dir() && !metadata.is_file() {
        // Admitted Nix NARs may have a regular file as their root. Native
        // registration keeps its existing output-shape law elsewhere.
        Some("canonical CAS path is not a directory, regular file, or symlink")
    } else {
        None
    };
    if let Some(reason) = reason {
        return Err(std::io::Error::other(format!(
            "admitted Nix entry `{}` is not its verified canonical CAS object: {reason}",
            entry.out
        )));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn register_entry_unlocked_with_hook(
    roots: &Roots,
    entry: &StoreEntry,
    after_append: &mut dyn FnMut(),
) -> std::io::Result<bool> {
    register_entry_unlocked_mode(roots, entry, Some(after_append), None)
}

#[cfg(test)]
fn register_entry_unlocked_mode(
    roots: &Roots,
    entry: &StoreEntry,
    after_append: Option<&mut dyn FnMut()>,
    fresh_action_key: Option<&str>,
) -> std::io::Result<bool> {
    Ingest::share_tree_files(
        roots,
        Path::new(&entry.out),
        !entry.platform_artifact_kind.is_empty(),
    )?;
    if object_digest_for_path(Path::new(&entry.out), &roots.hangar_dir()).as_deref()
        == Some(entry.envelope.output_hash.as_str())
    {
        write_seal(
            Path::new(&entry.out),
            &roots.hangar_dir(),
            &entry.envelope.output_hash,
        )?;
    }
    verify_registration_output(roots, entry)?;
    let (_, graph) = migrate_closure_graph_unlocked(roots)?;
    if let Some(action_key) = fresh_action_key {
        super::Reproducibility::certify_registration_unlocked_with_fresh_agreement(
            roots,
            entry,
            &[],
            action_key,
        )?;
    } else {
        super::Reproducibility::certify_registrations_unlocked(roots, std::slice::from_ref(entry))?;
    }
    let (objects, record) = descriptor_for_entry(roots, entry)?;
    if graph.records.get(&record.id) == Some(&record)
        && objects
            .iter()
            .all(|object| graph.objects.get(&object.digest) == Some(object))
    {
        return Ok(false);
    }
    let transaction = JournalEntry {
        kind: JournalKind::Delta,
        objects,
        records: vec![record],
        deleted_records: Vec::new(),
    };
    let mut candidate = graph;
    apply_entry(&mut candidate, transaction.clone()).map_err(std::io::Error::other)?;
    validate_graph_structure_mode(roots, &candidate, false).map_err(std::io::Error::other)?;
    validate_record_store_proof(roots, &transaction.records[0], false)
        .map_err(std::io::Error::other)?;
    let committed = append_entry(roots, &transaction)?;
    if let Some(after_append) = after_append {
        after_append();
    }
    if let Err(error) = verify_registration_output(roots, entry) {
        fs::remove_file(committed)?;
        sync_dir(&journal_dir(roots))?;
        return Err(error);
    }
    // The durable journal append plus post-commit content proof is the commit
    // point. Projection and compaction are recoverable maintenance; they must
    // not turn a committed registration into a reported failure.
    let _ = materialize_package_record(roots, &transaction.records[0]);
    let _ = compact_if_needed(roots);
    Ok(true)
}

fn verify_registration_output(roots: &Roots, entry: &StoreEntry) -> std::io::Result<()> {
    let actual = Ingest::try_entry_output_hash(roots, entry).map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("cannot register output `{}`: {error}", entry.out),
        )
    })?;
    if entry.envelope.output_hash.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "cannot register output `{}` without a content digest",
                entry.out
            ),
        ));
    }
    if actual != entry.envelope.output_hash {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "cannot register output `{}`: expected `{}`, got `{actual}`",
                entry.out, entry.envelope.output_hash
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn rollback_registration_dir(
    dir: &Path,
    created_dir: bool,
    had_gc_root: bool,
) -> std::io::Result<()> {
    let gc_root = dir.join(NIX_GC_ROOT);
    if !had_gc_root && fs::symlink_metadata(&gc_root).is_ok() {
        fs::remove_file(gc_root)?;
    }
    if created_dir {
        fs::remove_dir_all(dir)?;
    }
    Ok(())
}

/// Test-only seam (card #650): backdate a hangar entry's `last_used_at` in
/// the closure journal — not just `meta.json` — so a staleness/GC test can
/// simulate an old entry without waiting real time. A plain `meta.json`
/// edit doesn't survive: `recover_closure_journal_graph_unlocked` (run at
/// the top of every hangar operation, including the `jetpack hangar clean` a test
/// is about to invoke) re-materializes `meta.json` from the journal's
/// stored record, clobbering any out-of-band file edit first. This instead
/// re-registers the SAME entry (identical producer record, digests,
/// references — only `last_used_at` changes) through
/// `register_entry_unlocked`, the same path every real registration uses,
/// so it re-runs full structure/producer-record/digest/closure-proof
/// validation — there is no bypass here, just a different timestamp on an
/// otherwise-identical, still-valid record. Compiled only under the
/// `test-seam` feature (never enabled by a release build), so no
/// production caller can reach it.
#[cfg(feature = "test-seam")]
pub fn test_backdate_last_used_at(
    roots: &Roots,
    id: &str,
    last_used_at: u64,
) -> std::io::Result<()> {
    super::super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        recover_closure_journal_unlocked(roots)?;
        let mut entry = list_unlocked(roots)?
            .into_iter()
            .find(|entry| entry.id == id)
            .ok_or_else(|| std::io::Error::other(format!("no hangar entry `{id}`")))?;
        entry.last_used_at = last_used_at;
        register_entry_unlocked(roots, &entry)?;
        Ok(())
    })
}

pub fn remove_closure_record(roots: &Roots, id: &str) -> std::io::Result<bool> {
    super::super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        recover_closure_journal_unlocked(roots)?;
        if !load_graph(roots)?.records.contains_key(id) {
            return Ok(false);
        }
        append_entry(
            roots,
            &JournalEntry {
                kind: JournalKind::Delta,
                objects: Vec::new(),
                records: Vec::new(),
                deleted_records: vec![id.to_string()],
            },
        )?;
        remove_package_record(roots, id)?;
        compact_if_needed(roots)?;
        Ok(true)
    })
}

pub(super) fn tombstone_closure_record_unlocked(roots: &Roots, id: &str) -> std::io::Result<bool> {
    let graph = load_graph_structure_mode(roots, true)?;
    if !graph.records.contains_key(id) {
        return Ok(false);
    }
    append_entry(
        roots,
        &JournalEntry {
            kind: JournalKind::Delta,
            objects: Vec::new(),
            records: Vec::new(),
            deleted_records: vec![id.to_string()],
        },
    )?;
    compact_if_needed(roots)?;
    Ok(true)
}

pub fn recover_closure_journal(roots: &Roots) -> std::io::Result<usize> {
    super::super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        recover_closure_journal_unlocked(roots)
    })
}

pub(super) fn recover_closure_journal_unlocked(roots: &Roots) -> std::io::Result<usize> {
    recover_closure_journal_graph_unlocked(roots).map(|(recovered, _)| recovered)
}

/// Return the cached closure graph when the WAL identity stamp still matches.
/// A fresh stamp proves the journal bytes were already replayed and validated
/// under the lock in this process; a crash artifact (partial txn, staging
/// file) or any journal mutation changes the stamp. Projection repair is NOT
/// covered by the stamp — meta content drifts without a stamp change — so
/// callers must still run the repair loop.
fn fresh_cached_graph(roots: &Roots) -> Option<ClosureGraph> {
    let stamp = wal_state_stamp(roots).ok()?;
    let cache = STRUCTURE_GRAPH_CACHE.lock().ok()?;
    let (cached_stamp, graph) = cache.get(&roots.root)?;
    (*cached_stamp == stamp).then(|| graph.clone())
}

fn recover_closure_journal_graph_unlocked(roots: &Roots) -> std::io::Result<(usize, ClosureGraph)> {
    let fresh_graph = fresh_cached_graph(roots);
    let mut recovered = recover_receipt_staging(roots)?;
    let journal = journal_dir(roots);
    let Ok(entries) = fs::read_dir(&journal) else {
        return Ok((recovered, ClosureGraph::default()));
    };
    if fresh_graph.is_none() {
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(PARTIAL_SUFFIX) {
                if path.is_dir() {
                    fs::remove_dir_all(&path)?;
                } else {
                    fs::remove_file(&path)?;
                }
                recovered += 1;
            } else if name.ends_with(TXN_SUFFIX) {
                parse_entry(&fs::read_to_string(&path)?).map_err(|error| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("closure journal `{}`: {error}", path.display()),
                    )
                })?;
            }
        }
    }
    let graph = match fresh_graph {
        Some(graph) => graph,
        None => load_graph_structure_mode(roots, true)?,
    };
    for record in graph
        .records
        .values()
        .filter(|record| !record.package_meta.is_empty())
    {
        let meta = parse_meta(&record.package_meta).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "closure record `{}` has invalid package metadata",
                    record.id
                ),
            )
        })?;
        recovered +=
            usize::from(materialize_receipt(roots, &store_entry_from_meta(&record.id, &meta))?.1);
        recovered += usize::from(materialize_package_record(roots, record)?);
    }
    for id in &graph.deleted_records {
        recovered += usize::from(remove_package_record(roots, id)?);
    }
    Ok((recovered, graph))
}

fn descriptor_for_entry(
    roots: &Roots,
    entry: &StoreEntry,
) -> std::io::Result<(Vec<ClosureObject>, ClosureRecord)> {
    let mut normalized = entry.clone();
    // The package projection is a cache of the canonical receipt. Rebuild it
    // from the current facts at migration/registration boundaries; stale
    // projections must not prevent safe repair of the live output.
    normalized.receipt.clear();
    prepare_entry_receipt(roots, &mut normalized)?;
    let entry = &normalized;
    let primary = entry.envelope.output_hash.clone();
    if primary.is_empty() {
        return Err(std::io::Error::other(format!(
            "closure record `{}` has no output digest",
            entry.id
        )));
    }
    let object_root = roots.hangar_dir().join(OBJECTS_DIR);
    if let Some(named_primary) = entry.named_outputs.get("out") {
        if named_primary != &primary {
            return Err(std::io::Error::other(format!(
                "closure record `{}` names `out` as `{named_primary}`, not primary `{primary}`",
                entry.id
            )));
        }
    }
    let mut outputs = entry.named_outputs.clone();
    outputs.insert("out".to_string(), primary.clone());
    let producer = ProducerRecord::decode(&entry.producer_record).ok();
    let mut objects = Vec::new();
    for (name, digest) in &outputs {
        let path = if producer
            .as_ref()
            .is_some_and(|producer| producer.provider == "nix")
        {
            object_root.join(digest)
        } else if name == "out" {
            PathBuf::from(&entry.out)
        } else if let Some(producer) = producer.as_ref() {
            producer
                .facts
                .get(&format!("nix.output.{name}"))
                .or_else(|| producer.facts.get(&format!("output.path.{name}")))
                .map(PathBuf::from)
                .unwrap_or_else(|| object_root.join(digest))
        } else {
            object_root.join(digest)
        };
        let external = !path.starts_with(&roots.hangar_dir());
        objects.push(ClosureObject {
            digest: digest.clone(),
            path: path.to_string_lossy().into_owned(),
            external,
        });
    }
    if let Ok(producer) = ProducerRecord::decode(&entry.producer_record) {
        for digest in &entry.references {
            let Some(relative) = producer.facts.get(&format!("dependency.object.{digest}")) else {
                continue;
            };
            let relative = Path::new(relative);
            if relative.is_absolute()
                || relative
                    .components()
                    .any(|component| !matches!(component, std::path::Component::Normal(_)))
            {
                return Err(std::io::Error::other(format!(
                    "closure record `{}` has invalid dependency object path `{}`",
                    entry.id,
                    relative.display()
                )));
            }
            let path = Path::new(&entry.out).join(relative);
            let actual = Ingest::verified_file_hash(&path).map_err(|error| {
                std::io::Error::new(
                    error.kind(),
                    format!("reading dependency object `{}`: {error}", path.display()),
                )
            })?;
            if &actual != digest {
                return Err(std::io::Error::other(format!(
                    "dependency object `{}` records `{digest}`, re-hash produced `{actual}`",
                    path.display()
                )));
            }
            objects.push(ClosureObject {
                digest: digest.clone(),
                external: !path.starts_with(roots.hangar_dir()),
                path: path.to_string_lossy().into_owned(),
            });
        }
    }
    objects.sort_by(|left, right| left.digest.cmp(&right.digest));
    objects.dedup_by(|left, right| left.digest == right.digest);
    let mut package_entry = entry.clone();
    package_entry.named_outputs = outputs.clone();
    Ok((
        objects,
        ClosureRecord {
            id: entry.id.clone(),
            primary,
            action_key: entry_action_key(entry),
            outputs,
            references: entry.references.iter().cloned().collect(),
            producer_record: entry.producer_record.clone(),
            package_meta: package_entry.meta_json(),
        },
    ))
}

fn normalize_legacy_entry(mut entry: StoreEntry) -> std::io::Result<StoreEntry> {
    if !entry.producer_record.is_empty() {
        ProducerRecord::decode(&entry.producer_record).map_err(std::io::Error::other)?;
        return Ok(entry);
    }
    let identity = &entry.cache_identity;
    if identity.source_fingerprint.is_empty()
        || identity.recipe_fingerprint.is_empty()
        || identity.policy_fingerprint.is_empty()
        || identity.platform.is_empty()
        || entry.envelope.output_hash.is_empty()
    {
        return Err(std::io::Error::other(format!(
            "legacy package `{}` lacks immutable producer facts",
            entry.id
        )));
    }
    if entry.out.starts_with("/nix/store/") {
        return Err(std::io::Error::other(format!(
            "legacy Nix package `{}` lacks exact derivation facts",
            entry.id
        )));
    }
    let mut producer = ProducerRecord::decode(&canonical_producer(
        "legacy-migration",
        &format!("cas:{}", identity.source_fingerprint),
        &identity.source_fingerprint,
        identity,
        BTreeMap::from([("legacy.reference".into(), entry.reference.clone())]),
    )?)
    .map_err(std::io::Error::other)?;
    producer.bind_cache_provenance(
        &entry.reference,
        &entry.envelope.output_hash,
        identity,
        &entry.references,
    );
    super::super::Provider::refresh_provider_facts(&mut producer, &entry.reference)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    entry.producer_record = producer.encode();
    Ok(entry)
}
