use super::*;
use jet_codegen::development_receipt::{
    is_content_address, jet_development_receipt_render, JetDevelopmentReceipt,
    JetDevelopmentReceiptInput,
};
use std::io::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};

const DB_DIR: &str = "closure-db";
const JOURNAL_DIR: &str = "journal";
const PARTIAL_SUFFIX: &str = ".partial";
const TXN_SUFFIX: &str = ".txn";
const COMPACT_AFTER: usize = 64;
pub(crate) const RECEIPTS_DIR: &str = "receipts";
const RECEIPT_PARTIAL_SUFFIX: &str = ".partial";
const MAX_CLOSURE_OBJECTS: usize = 1_000_000;
const MAX_CLOSURE_RECORDS: usize = 1_000_000;
const MAX_CLOSURE_DELETIONS: usize = 1_000_000;
const MAX_CLOSURE_TRANSACTIONS: usize = 100_000;
static RECEIPT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

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

/// Total bytes on disk of a directory tree (0 if it isn't a local directory).
pub(crate) fn dir_size(path: &std::path::Path) -> u64 {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return 0;
    };
    if metadata.file_type().is_symlink() {
        return 0;
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
        assert!(text.contains("act\t\t7061636b6167652d7265616c697a6174696f6e\t"));
        assert!(text.contains("closure\t\t7368613235362d"));
        assert!(text.contains("action\t706c616e6e6564\t"));
        assert!(text.contains("activation-proof\t\t\n"));
        assert!(text.contains("witness\t\t"));
        assert!(text.contains("outcome\t\t706173736564\t"));
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

fn validate_universe_isolation(graph: &ClosureGraph, allow_legacy: bool) -> Result<(), String> {
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
struct CanonicalActionRecord {
    outputs: BTreeMap<String, String>,
    references: BTreeSet<String>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum JournalKind {
    Delta,
    Snapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct JournalEntry {
    kind: JournalKind,
    objects: Vec<ClosureObject>,
    records: Vec<ClosureRecord>,
    deleted_records: Vec<String>,
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

pub(super) fn closure_graph_structure_unlocked(roots: &Roots) -> std::io::Result<ClosureGraph> {
    migrate_closure_graph_unlocked(roots).map(|(_, graph)| graph)
}

pub(super) fn lifecycle_closure_graph_unlocked(roots: &Roots) -> std::io::Result<ClosureGraph> {
    let (_, graph) = migrate_closure_graph_unlocked(roots)?;
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

fn closure_object_rehashes(roots: &Roots, graph: &ClosureGraph, digest: &str) -> bool {
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
        if !Ingest::try_entry_output_hash(roots, &store_entry_from_meta(&record.id, &meta))
            .is_ok_and(|actual| actual == digest)
        {
            return false;
        }
    }
    if found_owner {
        return true;
    }
    let path = Path::new(&object.path);
    if path.is_file() {
        return SHA256::sha256_file_hex(path)
            .is_ok_and(|actual| format!("sha256-{actual}") == digest);
    }
    super::super::Envelope::try_output_hash_of_in_hangar(&object.path, &roots.hangar_dir(), false)
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

pub(super) fn migrate_closure_graph_unlocked(
    roots: &Roots,
) -> std::io::Result<(usize, ClosureGraph)> {
    let (_, mut graph) = recover_closure_journal_graph_unlocked(roots)?;
    let mut entries = list_unlocked(roots)?;
    entries.sort_by(|left, right| left.id.cmp(&right.id));
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

pub(crate) fn register_entry_unlocked(roots: &Roots, entry: &StoreEntry) -> std::io::Result<bool> {
    register_entry_unlocked_mode(roots, entry, None, None)
}

pub(crate) fn register_entry_unlocked_after_fresh_agreement(
    roots: &Roots,
    entry: &StoreEntry,
    action_key: &str,
) -> std::io::Result<bool> {
    register_entry_unlocked_mode(roots, entry, None, Some(action_key))
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
    if entries.is_empty() {
        return Ok(false);
    }
    for entry in entries {
        Ingest::share_tree_files(
            roots,
            Path::new(&entry.out),
            !entry.platform_artifact_kind.is_empty(),
        )?;
        verify_registration_output(roots, entry)?;
    }
    let (_, graph) = migrate_closure_graph_unlocked(roots)?;
    for entry in entries {
        super::certify_registration_unlocked(roots, entry, entries)?;
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

#[cfg(test)]
pub(crate) fn register_entry_unlocked_with_hook(
    roots: &Roots,
    entry: &StoreEntry,
    after_append: &mut dyn FnMut(),
) -> std::io::Result<bool> {
    register_entry_unlocked_mode(roots, entry, Some(after_append), None)
}

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
    verify_registration_output(roots, entry)?;
    let (_, graph) = migrate_closure_graph_unlocked(roots)?;
    if let Some(action_key) = fresh_action_key {
        super::certify_registration_unlocked_with_fresh_agreement(roots, entry, &[], action_key)?;
    } else {
        super::certify_registration_unlocked(roots, entry, &[])?;
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

fn recover_closure_journal_graph_unlocked(roots: &Roots) -> std::io::Result<(usize, ClosureGraph)> {
    let mut recovered = recover_receipt_staging(roots)?;
    let journal = journal_dir(roots);
    let Ok(entries) = fs::read_dir(&journal) else {
        return Ok((recovered, ClosureGraph::default()));
    };
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
    let graph = load_graph_structure_mode(roots, true)?;
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

/// Prepare the immutable receipt before the closure WAL becomes authoritative.
/// The caller keeps the returned digest in the package projection, so the
/// in-memory result, `meta.json`, closure record, and project lock all name the
/// same object.
pub(super) fn prepare_entry_receipt(roots: &Roots, entry: &mut StoreEntry) -> std::io::Result<()> {
    let (digest, _) = materialize_receipt(roots, entry)?;
    entry.receipt = digest;
    Ok(())
}

/// Returns the receipt digest and whether this call actually wrote it. The
/// recovery counter needs the second half: an already-present receipt is not a
/// recovery, and the digest alone cannot tell the two apart.
fn materialize_receipt(roots: &Roots, entry: &StoreEntry) -> std::io::Result<(String, bool)> {
    let bytes = render_receipt(entry).into_bytes();
    let digest = format!("sha256-{}", SHA256::sha256_hex(&bytes));
    if !entry.receipt.is_empty() && entry.receipt != digest {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "Hangar receipt projection for `{}` names `{}`, expected `{}`",
                entry.id, entry.receipt, digest
            ),
        ));
    }
    let receipts = roots.hangar_dir().join(RECEIPTS_DIR);
    super::Ingest::ensure_real_directory(&receipts, "Hangar receipt directory")?;
    let path = receipts.join(&digest);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Hangar receipt `{digest}` is not a regular file; repair the receipt object"
                ),
            ));
        }
        Ok(_) => {
            let actual = fs::read(&path)?;
            if actual != bytes {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "Hangar receipt `{digest}` is corrupt; restore the exact object or remove the unreferenced receipt"
                    ),
                ));
            }
            return Ok((digest, false));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }

    let sequence = RECEIPT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let partial = receipts.join(format!(
        ".{digest}-{}-{sequence}{RECEIPT_PARTIAL_SUFFIX}",
        std::process::id()
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)?;
    if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
        let _ = fs::remove_file(&partial);
        return Err(error);
    }
    match fs::rename(&partial, &path) {
        Ok(()) => {
            sync_dir(&receipts)?;
            Ok((digest, true))
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(&partial);
            let actual = fs::read(&path)?;
            if actual != bytes {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Hangar receipt `{digest}` changed during publication"),
                ));
            }
            Ok((digest, false))
        }
        Err(error) => {
            let _ = fs::remove_file(&partial);
            Err(error)
        }
    }
}

fn recover_receipt_staging(roots: &Roots) -> std::io::Result<usize> {
    let receipts = roots.hangar_dir().join(RECEIPTS_DIR);
    let Ok(metadata) = fs::symlink_metadata(&receipts) else {
        return Ok(0);
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Hangar receipt directory is not a real directory; repair the path before recovery",
        ));
    }
    let mut recovered = 0;
    for item in fs::read_dir(&receipts)? {
        let path = item?.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if !name.starts_with('.') || !name.ends_with(RECEIPT_PARTIAL_SUFFIX) {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || metadata.is_file() {
            fs::remove_file(&path)?;
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Hangar receipt staging path `{}` is not removable",
                    path.display()
                ),
            ));
        }
        recovered += 1;
    }
    if recovered > 0 {
        sync_dir(&receipts)?;
    }
    Ok(recovered)
}

fn render_receipt(entry: &StoreEntry) -> String {
    let mut inputs = vec![
        receipt_input("package-name", &entry.name),
        receipt_input("package-version", &entry.version),
        receipt_input("reference", &entry.reference),
        receipt_input(
            "source-fingerprint",
            &entry.cache_identity.source_fingerprint,
        ),
        receipt_input(
            "recipe-fingerprint",
            &entry.cache_identity.recipe_fingerprint,
        ),
        receipt_input(
            "policy-fingerprint",
            &entry.cache_identity.policy_fingerprint,
        ),
        receipt_input("platform", &entry.cache_identity.platform),
        receipt_input("platform-artifact-kind", &entry.platform_artifact_kind),
        receipt_input("producer-record", &entry.producer_record),
    ];
    let references = entry.references.iter().collect::<BTreeSet<_>>();
    for reference in references {
        inputs.push(receipt_input("closure", reference));
    }
    let action = entry_action_key(entry);
    let mut outputs = entry.named_outputs.clone();
    outputs.insert("out".to_string(), entry.envelope.output_hash.clone());
    let outputs = outputs
        .into_iter()
        .map(|(name, digest)| receipt_input(&name, &digest))
        .collect();
    let receipt = JetDevelopmentReceipt {
        act: "package-realization".into(),
        locked_closure: action.clone(),
        inputs,
        planned_action: action,
        outputs,
        activation_proof: String::new(),
        parent_generation: String::new(),
        witness: receipt_witness(),
        outcome: "passed".into(),
        failure_path: None,
    };
    jet_development_receipt_render(&receipt)
}

fn receipt_input(name: &str, value: &str) -> JetDevelopmentReceiptInput {
    let digest = if is_content_address(value) {
        value.to_owned()
    } else {
        format!("sha256-{}", SHA256::sha256_hex(value.as_bytes()))
    };
    JetDevelopmentReceiptInput {
        name: name.to_owned(),
        digest,
    }
}

fn receipt_witness() -> String {
    std::env::var("JET_RECEIPT_WITNESS")
        .ok()
        .filter(|witness| !witness.is_empty())
        .unwrap_or_else(|| "jetpack".into())
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
            let bytes = fs::read(&path).map_err(|error| {
                std::io::Error::new(
                    error.kind(),
                    format!("reading dependency object `{}`: {error}", path.display()),
                )
            })?;
            let actual = format!("sha256-{}", SHA256::sha256_hex(&bytes));
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

fn load_graph(roots: &Roots) -> std::io::Result<ClosureGraph> {
    load_graph_mode(roots, false)
}

/// Validate committed closure state without locking, replaying, compacting, or
/// repairing its package projection.
pub(crate) fn closure_graph_read_only(roots: &Roots) -> std::io::Result<ClosureGraph> {
    let journal = journal_dir(roots);
    match fs::read_dir(&journal) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                if entry
                    .file_name()
                    .to_string_lossy()
                    .ends_with(PARTIAL_SUFFIX)
                {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "closure journal contains an incomplete transaction",
                    ));
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ClosureGraph::default());
        }
        Err(error) => return Err(error),
    }
    load_graph_mode(roots, true)
}

/// Read the closure structure without taking a lock or replaying a journal.
/// Store proofs may be unavailable, but an explanation must never repair the
/// projection merely to describe that loss.
pub(crate) fn closure_graph_structure_read_only(roots: &Roots) -> std::io::Result<ClosureGraph> {
    let journal = journal_dir(roots);
    match fs::read_dir(&journal) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                if entry
                    .file_name()
                    .to_string_lossy()
                    .ends_with(PARTIAL_SUFFIX)
                {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "closure journal contains an incomplete transaction",
                    ));
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ClosureGraph::default());
        }
        Err(error) => return Err(error),
    }
    load_graph_structure_mode(roots, true)
}

pub(super) fn lifecycle_inputs_unlocked(
    roots: &Roots,
) -> std::io::Result<(BTreeSet<String>, String)> {
    recover_closure_journal_unlocked(roots)?;
    let graph = load_graph_mode(roots, true)?;
    let journal = journal_dir(roots);
    let mut paths = transaction_paths(&journal)?;
    paths.sort();
    let mut canonical = b"jet-closure-head-v1\0".to_vec();
    for path in paths {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| std::io::Error::other("closure journal has a non-UTF-8 name"))?;
        let bytes = fs::read(&path)?;
        canonical.extend_from_slice(&(name.len() as u64).to_be_bytes());
        canonical.extend_from_slice(name.as_bytes());
        canonical.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
        canonical.extend_from_slice(&bytes);
    }
    Ok((
        graph.objects.into_keys().collect(),
        format!("sha256-{}", SHA256::sha256_hex(&canonical)),
    ))
}

fn load_graph_mode(roots: &Roots, allow_legacy: bool) -> std::io::Result<ClosureGraph> {
    load_graph_mode_with_proofs(roots, allow_legacy, true)
}

fn load_graph_structure_mode(roots: &Roots, allow_legacy: bool) -> std::io::Result<ClosureGraph> {
    load_graph_mode_with_proofs(roots, allow_legacy, false)
}

fn load_graph_mode_with_proofs(
    roots: &Roots,
    allow_legacy: bool,
    validate_store_proofs: bool,
) -> std::io::Result<ClosureGraph> {
    let journal = journal_dir(roots);
    let Ok(entries) = fs::read_dir(&journal) else {
        return Ok(ClosureGraph::default());
    };
    let mut paths = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("txn") {
            if paths.len() >= MAX_CLOSURE_TRANSACTIONS {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("closure journal exceeds {MAX_CLOSURE_TRANSACTIONS} transactions"),
                ));
            }
            paths.push(path);
        }
    }
    paths.sort();
    let mut graph = ClosureGraph::default();
    for path in paths {
        let entry = parse_entry(&fs::read_to_string(&path)?).map_err(|error| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("closure journal `{}`: {error}", path.display()),
            )
        })?;
        let applied = entry.clone();
        apply_entry(&mut graph, entry)
            .and_then(|()| validate_applied_entry(roots, &graph, &applied, allow_legacy))
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    }
    validate_graph_structure_mode(roots, &graph, allow_legacy)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    if validate_store_proofs {
        validate_graph_store_proofs(roots, &graph, allow_legacy)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    }
    Ok(graph)
}

fn apply_entry(graph: &mut ClosureGraph, entry: JournalEntry) -> Result<(), String> {
    if entry.objects.len() > MAX_CLOSURE_OBJECTS {
        return Err(format!(
            "closure transaction exceeds {MAX_CLOSURE_OBJECTS} objects"
        ));
    }
    if entry.records.len() > MAX_CLOSURE_RECORDS {
        return Err(format!(
            "closure transaction exceeds {MAX_CLOSURE_RECORDS} records"
        ));
    }
    if entry.deleted_records.len() > MAX_CLOSURE_DELETIONS {
        return Err(format!(
            "closure transaction exceeds {MAX_CLOSURE_DELETIONS} deleted records"
        ));
    }
    if entry.kind != JournalKind::Snapshot {
        let new_objects = entry
            .objects
            .iter()
            .filter(|object| !graph.objects.contains_key(&object.digest))
            .count();
        if graph.objects.len().saturating_add(new_objects) > MAX_CLOSURE_OBJECTS {
            return Err(format!(
                "closure graph exceeds {MAX_CLOSURE_OBJECTS} objects"
            ));
        }
        let deleted_records = entry
            .deleted_records
            .iter()
            .filter(|id| graph.records.contains_key(*id))
            .count();
        let deleted_record_ids = entry
            .deleted_records
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let mut record_count = graph.records.len().saturating_sub(deleted_records);
        for record in &entry.records {
            if !graph.records.contains_key(&record.id)
                || deleted_record_ids.contains(record.id.as_str())
            {
                record_count = record_count.saturating_add(1);
            }
        }
        if record_count > MAX_CLOSURE_RECORDS {
            return Err(format!(
                "closure graph exceeds {MAX_CLOSURE_RECORDS} records"
            ));
        }
        if graph
            .deleted_records
            .len()
            .saturating_add(entry.deleted_records.len())
            > MAX_CLOSURE_DELETIONS
        {
            return Err(format!(
                "closure graph exceeds {MAX_CLOSURE_DELETIONS} deleted records"
            ));
        }
    }
    if entry.kind == JournalKind::Snapshot {
        *graph = ClosureGraph::default();
    }
    for id in entry.deleted_records {
        graph.records.remove(&id);
        graph.deleted_records.insert(id);
    }
    for object in entry.objects {
        if let Some(existing) = graph.objects.get(&object.digest) {
            if existing != &object {
                return Err(format!(
                    "closure object `{}` changed immutable descriptor",
                    object.digest
                ));
            }
        } else {
            graph.objects.insert(object.digest.clone(), object);
        }
    }
    for record in entry.records {
        reject_action_conflict(graph, &record)?;
        graph.deleted_records.remove(&record.id);
        graph.records.insert(record.id.clone(), record);
    }
    Ok(())
}

#[cfg(test)]
fn validate_graph(roots: &Roots, graph: &ClosureGraph) -> Result<(), String> {
    validate_graph_mode(roots, graph, false)
}

#[cfg(test)]
fn validate_graph_mode(
    roots: &Roots,
    graph: &ClosureGraph,
    allow_legacy: bool,
) -> Result<(), String> {
    validate_graph_structure_mode(roots, graph, allow_legacy)?;
    validate_graph_store_proofs(roots, graph, allow_legacy)
}

fn validate_graph_structure_mode(
    roots: &Roots,
    graph: &ClosureGraph,
    allow_legacy: bool,
) -> Result<(), String> {
    let hangar = roots.hangar_dir();
    for (digest, object) in &graph.objects {
        if digest.is_empty() || digest != &object.digest || object.path.is_empty() {
            return Err(format!("invalid closure object descriptor `{digest}`"));
        }
        let path = Path::new(&object.path);
        if path
            .components()
            .any(|component| component == std::path::Component::ParentDir)
        {
            return Err(format!(
                "closure object `{digest}` path contains parent traversal"
            ));
        }
        let expected_external = !path.starts_with(&hangar);
        if object.external != expected_external {
            return Err(format!(
                "closure object `{digest}` has invalid external marker"
            ));
        }
    }
    if let Some(id) = graph
        .deleted_records
        .iter()
        .find(|id| id.is_empty() || graph.records.contains_key(*id))
    {
        return Err(format!("invalid deleted closure record `{id}`"));
    }
    let mut actions: BTreeMap<&str, CanonicalActionRecord> = BTreeMap::new();
    for (id, record) in &graph.records {
        if id.is_empty() || id != &record.id || record.action_key.is_empty() {
            return Err(format!("invalid closure record `{id}`"));
        }
        if record.outputs.get("out") != Some(&record.primary) {
            return Err(format!(
                "closure record `{id}` primary is not its `out` output"
            ));
        }
        for (name, digest) in &record.outputs {
            if !valid_output_name(name) {
                return Err(format!(
                    "closure record `{id}` has invalid output name `{name}`"
                ));
            }
            if !graph.objects.contains_key(digest) {
                return Err(format!(
                    "closure record `{id}` output `{name}` references missing object `{digest}`"
                ));
            }
        }
        if let Some(missing) = record
            .references
            .iter()
            .find(|digest| !graph.objects.contains_key(*digest))
        {
            return Err(format!(
                "closure record `{id}` references missing object `{missing}`"
            ));
        }
        let legacy = record.producer_record.is_empty() && record.package_meta.is_empty();
        if !allow_legacy || !legacy {
            ProducerRecord::decode(&record.producer_record).map_err(|error| {
                format!("closure record `{id}` has invalid producer record: {error}")
            })?;
            let meta = parse_meta(&record.package_meta)
                .ok_or_else(|| format!("closure record `{id}` has invalid package metadata"))?;
            validate_receipt_projection(roots, id, &meta, allow_legacy)?;
            if record.action_key != entry_action_key(&store_entry_from_meta(id, &meta)) {
                return Err(format!(
                    "closure record `{id}` action key disagrees with package metadata"
                ));
            }
            if meta.producer_record != record.producer_record {
                return Err(format!(
                    "closure record `{id}` package metadata disagrees with producer record"
                ));
            }
            if meta.envelope.output_hash != record.primary || meta.named_outputs != record.outputs {
                return Err(format!(
                    "closure record `{id}` package metadata disagrees with outputs"
                ));
            }
        }
        let projection = canonical_action_projection(record)?;
        if let Some(action) = actions.get_mut(record.action_key.as_str()) {
            merge_action_projection(action, &projection, &record.action_key)?;
        } else {
            actions.insert(record.action_key.as_str(), projection);
        }
    }
    validate_universe_isolation(graph, allow_legacy)?;
    Ok(())
}

fn validate_applied_entry(
    roots: &Roots,
    graph: &ClosureGraph,
    entry: &JournalEntry,
    allow_legacy: bool,
) -> Result<(), String> {
    if entry.kind == JournalKind::Snapshot {
        return validate_graph_structure_mode(roots, graph, allow_legacy);
    }
    let hangar = roots.hangar_dir();
    for id in &entry.deleted_records {
        if id.is_empty() || graph.records.contains_key(id) || !graph.deleted_records.contains(id) {
            return Err(format!("invalid deleted closure record `{id}`"));
        }
    }
    for object in &entry.objects {
        let digest = &object.digest;
        if digest.is_empty() || object.path.is_empty() {
            return Err(format!("invalid closure object descriptor `{digest}`"));
        }
        let path = Path::new(&object.path);
        if path
            .components()
            .any(|component| component == std::path::Component::ParentDir)
        {
            return Err(format!(
                "closure object `{digest}` path contains parent traversal"
            ));
        }
        if object.external != !path.starts_with(&hangar) {
            return Err(format!(
                "closure object `{digest}` has invalid external marker"
            ));
        }
    }
    for record in &entry.records {
        let id = &record.id;
        if id.is_empty() || record.action_key.is_empty() {
            return Err(format!("invalid closure record `{id}`"));
        }
        if record.outputs.get("out") != Some(&record.primary) {
            return Err(format!(
                "closure record `{id}` primary is not its `out` output"
            ));
        }
        for (name, digest) in &record.outputs {
            if !valid_output_name(name) {
                return Err(format!(
                    "closure record `{id}` has invalid output name `{name}`"
                ));
            }
            if !graph.objects.contains_key(digest) {
                return Err(format!(
                    "closure record `{id}` output `{name}` references missing object `{digest}`"
                ));
            }
        }
        if let Some(missing) = record
            .references
            .iter()
            .find(|digest| !graph.objects.contains_key(*digest))
        {
            return Err(format!(
                "closure record `{id}` references missing object `{missing}`"
            ));
        }
        let legacy = record.producer_record.is_empty() && record.package_meta.is_empty();
        if !allow_legacy || !legacy {
            ProducerRecord::decode(&record.producer_record).map_err(|error| {
                format!("closure record `{id}` has invalid producer record: {error}")
            })?;
            let meta = parse_meta(&record.package_meta)
                .ok_or_else(|| format!("closure record `{id}` has invalid package metadata"))?;
            validate_receipt_projection(roots, id, &meta, allow_legacy)?;
            if record.action_key != entry_action_key(&store_entry_from_meta(id, &meta)) {
                return Err(format!(
                    "closure record `{id}` action key disagrees with package metadata"
                ));
            }
            if meta.producer_record != record.producer_record {
                return Err(format!(
                    "closure record `{id}` package metadata disagrees with producer record"
                ));
            }
            if meta.envelope.output_hash != record.primary || meta.named_outputs != record.outputs {
                return Err(format!(
                    "closure record `{id}` package metadata disagrees with outputs"
                ));
            }
        }
        canonical_action_projection(record)?;
    }
    Ok(())
}

fn validate_graph_store_proofs(
    roots: &Roots,
    graph: &ClosureGraph,
    allow_legacy: bool,
) -> Result<(), String> {
    for record in graph.records.values() {
        if !record.package_meta.is_empty() {
            let meta = parse_meta(&record.package_meta).ok_or_else(|| {
                format!(
                    "closure record `{}` has invalid package metadata",
                    record.id
                )
            })?;
            validate_receipt_projection(roots, &record.id, &meta, allow_legacy)?;
        }
        validate_record_store_proof(roots, record, allow_legacy)?;
    }
    Ok(())
}

fn validate_receipt_projection(
    roots: &Roots,
    id: &str,
    meta: &ParsedMeta,
    allow_legacy: bool,
) -> Result<(), String> {
    if meta.receipt.is_empty() {
        if allow_legacy {
            return Ok(());
        }
        return Err(format!("closure record `{id}` has no Hangar receipt"));
    }
    if !valid_receipt_digest(&meta.receipt) {
        return Err(format!(
            "closure record `{id}` has an invalid Hangar receipt digest `{}`",
            meta.receipt
        ));
    }
    let entry = store_entry_from_meta(id, meta);
    let expected_bytes = render_receipt(&entry).into_bytes();
    let expected = format!("sha256-{}", SHA256::sha256_hex(&expected_bytes));
    if meta.receipt != expected {
        return Err(format!(
            "closure record `{id}` receipt digest `{}` disagrees with `{expected}`",
            meta.receipt
        ));
    }
    let path = roots.hangar_dir().join(RECEIPTS_DIR).join(&meta.receipt);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && allow_legacy => {
            // Locked recovery materializes a missing immutable object from the
            // authoritative closure record immediately after graph loading.
            return Ok(());
        }
        Err(error) => {
            return Err(format!(
                "closure record `{id}` cannot read Hangar receipt `{}`: {error}",
                path.display()
            ));
        }
    };
    if bytes != expected_bytes {
        return Err(format!("closure record `{id}` Hangar receipt is corrupt"));
    }
    Ok(())
}

fn validate_record_store_proof(
    roots: &Roots,
    record: &ClosureRecord,
    allow_legacy: bool,
) -> Result<(), String> {
    let legacy = record.producer_record.is_empty() && record.package_meta.is_empty();
    if allow_legacy && legacy || !record.references.is_empty() {
        return Ok(());
    }
    let producer = ProducerRecord::decode(&record.producer_record).map_err(|error| {
        format!(
            "closure record `{}` has invalid producer record: {error}",
            record.id
        )
    })?;
    let meta = parse_meta(&record.package_meta).ok_or_else(|| {
        format!(
            "closure record `{}` has invalid package metadata",
            record.id
        )
    })?;
    if store_validates_complete_closure(roots, record, &meta, &producer) {
        Ok(())
    } else {
        Err(format!(
            "closure record `{}` has no dependency references or store-validated closure proof",
            record.id
        ))
    }
}

fn valid_output_name(name: &str) -> bool {
    if name.bytes().any(|byte| matches!(byte, b'/' | b'\\')) {
        return false;
    }
    let mut components = Path::new(name).components();
    matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none()
}

fn reject_action_conflict(graph: &ClosureGraph, candidate: &ClosureRecord) -> Result<(), String> {
    let candidate_projection = canonical_action_projection(candidate)?;
    for existing in graph
        .records
        .values()
        .filter(|record| record.action_key == candidate.action_key)
    {
        let mut action = canonical_action_projection(existing)?;
        merge_action_projection(&mut action, &candidate_projection, &candidate.action_key)?;
    }
    Ok(())
}

fn canonical_action_projection(record: &ClosureRecord) -> Result<CanonicalActionRecord, String> {
    if record.producer_record.is_empty() {
        return Ok(CanonicalActionRecord {
            outputs: record.outputs.clone(),
            references: record.references.clone(),
        });
    }
    let producer = ProducerRecord::decode(&record.producer_record).map_err(|error| {
        format!(
            "closure record `{}` has invalid producer record: {error}",
            record.id
        )
    })?;
    let outputs: BTreeMap<String, String> = if producer.provider == "nix" {
        producer
            .facts
            .keys()
            .filter_map(|key| key.strip_prefix("nix.output."))
            .filter_map(|name| {
                record
                    .outputs
                    .get(name)
                    .map(|digest| (name.to_string(), digest.clone()))
            })
            .collect()
    } else {
        record.outputs.clone()
    };
    if outputs.is_empty() {
        return Err(format!(
            "closure record `{}` has no canonical action output projection",
            record.id
        ));
    }
    Ok(CanonicalActionRecord {
        outputs,
        references: record.references.clone(),
    })
}

fn merge_action_projection(
    action: &mut CanonicalActionRecord,
    projection: &CanonicalActionRecord,
    action_key: &str,
) -> Result<(), String> {
    if action.references != projection.references {
        return Err(format!(
            "action `{action_key}` has conflicting dependency references"
        ));
    }
    for (name, digest) in &projection.outputs {
        if let Some(existing) = action.outputs.get(name) {
            if existing != digest {
                return Err(format!(
                    "action `{action_key}` output `{name}` maps to conflicting bytes `{existing}` and `{digest}`"
                ));
            }
        } else {
            action.outputs.insert(name.clone(), digest.clone());
        }
    }
    Ok(())
}

fn store_validates_complete_closure(
    roots: &Roots,
    record: &ClosureRecord,
    meta: &ParsedMeta,
    producer: &ProducerRecord,
) -> bool {
    let output = Path::new(&meta.out);
    let local = roots.hangar_dir().join(OBJECTS_DIR).join(&record.primary);
    let authority = producer.facts.get("closure.authority").map(String::as_str);
    match producer.provider.as_str() {
        "core" => rehashes_as_recorded(roots, meta, record),
        "adapter" | "cran" | "luarocks" if output == local => {
            rehashes_as_recorded(roots, meta, record)
        }
        "hangar-ingest" if output == local && authority == Some("hangar-cas") => {
            rehashes_as_recorded(roots, meta, record)
        }
        "store-record" if authority == Some("hangar-cas") => {
            rehashes_as_recorded(roots, meta, record)
        }
        // A jetpackage release artifact is a complete native output with no
        // dependency edges. Store registration has already canonicalized it
        // into Hangar, so the same byte proof used by Core applies here.
        "jetpackage" if output == local => rehashes_as_recorded(roots, meta, record),
        "nix" if output == local => rehashes_as_recorded(roots, meta, record),
        "nix" if output.starts_with("/nix/store") => {
            let root = roots.hangar_dir().join(&record.id).join("nix-gc-root");
            root.exists() && std::fs::canonicalize(root).ok() == std::fs::canonicalize(output).ok()
        }
        _ => false,
    }
}

fn rehashes_as_recorded(roots: &Roots, meta: &ParsedMeta, record: &ClosureRecord) -> bool {
    super::super::Envelope::try_output_hash_of_in_hangar(
        &meta.out,
        &roots.hangar_dir(),
        !meta.platform_artifact_kind.is_empty(),
    )
    .ok()
    .as_ref()
        == Some(&record.primary)
}

fn store_entry_from_meta(id: &str, meta: &ParsedMeta) -> StoreEntry {
    StoreEntry {
        id: id.to_string(),
        name: meta.name.clone(),
        version: meta.version.clone(),
        reference: meta.reference.clone(),
        out: meta.out.clone(),
        bin: meta.bin.clone(),
        rlib: meta.rlib.clone(),
        envelope: meta.envelope.clone(),
        cache_identity: meta.cache_identity.clone(),
        references: meta.references.clone(),
        named_outputs: meta.named_outputs.clone(),
        platform_artifact_kind: meta.platform_artifact_kind.clone(),
        producer_record: meta.producer_record.clone(),
        receipt: meta.receipt.clone(),
        realized_at: meta.realized_at.unwrap_or(0),
        last_used_at: meta.last_used_at.unwrap_or(0),
    }
}

fn append_entry(roots: &Roots, entry: &JournalEntry) -> std::io::Result<PathBuf> {
    let journal = journal_dir(roots);
    ensure_directory_durable(&journal)?;
    let sequence = next_sequence(&journal)?;
    write_entry(&journal, sequence, entry)
}

fn compact_if_needed(roots: &Roots) -> std::io::Result<()> {
    let journal = journal_dir(roots);
    let mut paths = transaction_paths(&journal)?;
    if paths.len() <= COMPACT_AFTER {
        return Ok(());
    }
    let graph = load_graph_structure_mode(roots, false)?;
    let snapshot = JournalEntry {
        kind: JournalKind::Snapshot,
        objects: graph.objects.into_values().collect(),
        records: graph.records.into_values().collect(),
        deleted_records: graph.deleted_records.into_iter().collect(),
    };
    let sequence = next_sequence(&journal)?;
    write_entry(&journal, sequence, &snapshot)?;
    paths.sort();
    for path in paths {
        fs::remove_file(path)?;
    }
    sync_dir(&journal)
}

fn write_entry(journal: &Path, sequence: u64, entry: &JournalEntry) -> std::io::Result<PathBuf> {
    let text = render_entry(entry);
    let checksum = SHA256::sha256_hex(text.as_bytes());
    let final_path = journal.join(format!("{sequence:020}-{}.txn", &checksum[..16]));
    let partial = journal.join(format!("{sequence:020}-{}.partial", &checksum[..16]));
    fs::write(&partial, format!("{text}checksum\t{checksum}\n"))?;
    fs::File::open(&partial)?.sync_all()?;
    fs::rename(&partial, &final_path)?;
    sync_dir(journal)?;
    Ok(final_path)
}

fn materialize_package_record(roots: &Roots, record: &ClosureRecord) -> std::io::Result<bool> {
    let dir = roots.hangar_dir().join(&record.id);
    fs::create_dir_all(&dir)?;
    let path = dir.join("meta.json");
    if fs::read_to_string(&path).ok().as_deref() == Some(record.package_meta.as_str()) {
        return Ok(false);
    }
    let tmp = dir.join(format!("meta.json.{}.partial", std::process::id()));
    fs::write(&tmp, &record.package_meta)?;
    fs::File::open(&tmp)?.sync_all()?;
    #[cfg(windows)]
    if path.exists() {
        // std rename does not replace on Windows. The committed WAL remains
        // authoritative if a crash lands between removal and publication;
        // the next recovery recreates the exact projection.
        fs::remove_file(&path)?;
        sync_dir(&dir)?;
    }
    fs::rename(&tmp, &path)?;
    sync_dir(&dir)?;
    Ok(true)
}

fn remove_package_record(roots: &Roots, id: &str) -> std::io::Result<bool> {
    let dir = roots.hangar_dir().join(id);
    let path = dir.join("meta.json");
    if !path.exists() {
        return Ok(false);
    }
    fs::remove_file(path)?;
    sync_dir(&dir)?;
    Ok(true)
}

fn render_entry(entry: &JournalEntry) -> String {
    let mut out = String::from("jet-closure-journal-v1\n");
    out.push_str(match entry.kind {
        JournalKind::Delta => "kind\tdelta\n",
        JournalKind::Snapshot => "kind\tsnapshot\n",
    });
    let mut objects = entry.objects.clone();
    objects.sort_by(|left, right| left.digest.cmp(&right.digest));
    for object in objects {
        out.push_str(&format!(
            "object\t{}\t{}\t{}\n",
            hex(&object.digest),
            hex(&object.path),
            u8::from(object.external),
        ));
    }
    let mut records = entry.records.clone();
    records.sort_by(|left, right| left.id.cmp(&right.id));
    for record in records {
        out.push_str(&format!(
            "record\t{}\t{}\t{}\t{}\t{}\n",
            hex(&record.id),
            hex(&record.primary),
            hex(&record.action_key),
            hex(&record.producer_record),
            hex(&record.package_meta),
        ));
        for (name, digest) in record.outputs {
            out.push_str(&format!(
                "output\t{}\t{}\t{}\n",
                hex(&record.id),
                hex(&name),
                hex(&digest),
            ));
        }
        for reference in record.references {
            out.push_str(&format!(
                "reference\t{}\t{}\n",
                hex(&record.id),
                hex(&reference),
            ));
        }
    }
    let mut deleted = entry.deleted_records.clone();
    deleted.sort();
    for id in deleted {
        out.push_str(&format!("delete\t{}\n", hex(&id)));
    }
    out
}

fn parse_entry(raw: &str) -> Result<JournalEntry, String> {
    let Some((body, checksum_line)) = raw.rsplit_once("checksum\t") else {
        return Err("missing checksum".to_string());
    };
    let checksum = checksum_line
        .strip_suffix('\n')
        .ok_or_else(|| "truncated checksum frame".to_string())?;
    if checksum.len() != 64
        || !checksum
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err("invalid checksum frame".to_string());
    }
    if SHA256::sha256_hex(body.as_bytes()) != checksum {
        return Err("checksum mismatch".to_string());
    }
    let mut lines = body.lines();
    if lines.next() != Some("jet-closure-journal-v1") {
        return Err("unsupported journal version".to_string());
    }
    let kind = match lines.next() {
        Some("kind\tdelta") => JournalKind::Delta,
        Some("kind\tsnapshot") => JournalKind::Snapshot,
        _ => return Err("missing journal kind".to_string()),
    };
    let mut objects = Vec::new();
    let mut records: BTreeMap<String, ClosureRecord> = BTreeMap::new();
    let mut deleted_records = Vec::new();
    for line in lines {
        let fields = line.split('\t').collect::<Vec<_>>();
        match fields.as_slice() {
            ["object", digest, path, external @ ("0" | "1")] => {
                if objects.len() >= MAX_CLOSURE_OBJECTS {
                    return Err(format!(
                        "closure transaction exceeds {MAX_CLOSURE_OBJECTS} objects"
                    ));
                }
                objects.push(ClosureObject {
                    digest: unhex(digest)?,
                    path: unhex(path)?,
                    external: *external == "1",
                });
            }
            ["record", id, primary, action_key, producer_record, package_meta] => {
                let id = unhex(id)?;
                if records.contains_key(&id) {
                    return Err(format!("duplicate closure record `{id}`"));
                }
                if records.len() >= MAX_CLOSURE_RECORDS {
                    return Err(format!(
                        "closure transaction exceeds {MAX_CLOSURE_RECORDS} records"
                    ));
                }
                records.insert(
                    id.clone(),
                    ClosureRecord {
                        id,
                        primary: unhex(primary)?,
                        action_key: unhex(action_key)?,
                        outputs: BTreeMap::new(),
                        references: BTreeSet::new(),
                        producer_record: unhex(producer_record)?,
                        package_meta: unhex(package_meta)?,
                    },
                );
            }
            ["record", id, primary, action_key] => {
                let id = unhex(id)?;
                if records.contains_key(&id) {
                    return Err(format!("duplicate closure record `{id}`"));
                }
                if records.len() >= MAX_CLOSURE_RECORDS {
                    return Err(format!(
                        "closure transaction exceeds {MAX_CLOSURE_RECORDS} records"
                    ));
                }
                records.insert(
                    id.clone(),
                    ClosureRecord {
                        id,
                        primary: unhex(primary)?,
                        action_key: unhex(action_key)?,
                        outputs: BTreeMap::new(),
                        references: BTreeSet::new(),
                        producer_record: String::new(),
                        package_meta: String::new(),
                    },
                );
            }
            ["output", id, name, digest] => {
                let id = unhex(id)?;
                let record = records
                    .get_mut(&id)
                    .ok_or_else(|| format!("output precedes record `{id}`"))?;
                let name = unhex(name)?;
                if record.outputs.contains_key(&name) {
                    return Err(format!(
                        "duplicate output `{name}` in closure record `{id}`"
                    ));
                }
                record.outputs.insert(name, unhex(digest)?);
            }
            ["reference", id, digest] => {
                let id = unhex(id)?;
                let record = records
                    .get_mut(&id)
                    .ok_or_else(|| format!("reference precedes record `{id}`"))?;
                record.references.insert(unhex(digest)?);
            }
            ["delete", id] => {
                if deleted_records.len() >= MAX_CLOSURE_DELETIONS {
                    return Err(format!(
                        "closure transaction exceeds {MAX_CLOSURE_DELETIONS} deleted records"
                    ));
                }
                deleted_records.push(unhex(id)?);
            }
            _ => return Err(format!("invalid journal line `{line}`")),
        }
    }
    Ok(JournalEntry {
        kind,
        objects,
        records: records.into_values().collect(),
        deleted_records,
    })
}

fn next_sequence(journal: &Path) -> std::io::Result<u64> {
    Ok(transaction_paths(journal)?
        .iter()
        .filter_map(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.get(..20))
                .and_then(|sequence| sequence.parse::<u64>().ok())
        })
        .max()
        .unwrap_or(0)
        + 1)
}

fn transaction_paths(journal: &Path) -> std::io::Result<Vec<PathBuf>> {
    let Ok(entries) = fs::read_dir(journal) else {
        return Ok(Vec::new());
    };
    let mut paths = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("txn") {
            if paths.len() >= MAX_CLOSURE_TRANSACTIONS {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("closure journal exceeds {MAX_CLOSURE_TRANSACTIONS} transactions"),
                ));
            }
            paths.push(path);
        }
    }
    Ok(paths)
}

fn journal_dir(roots: &Roots) -> PathBuf {
    roots.hangar_dir().join(DB_DIR).join(JOURNAL_DIR)
}

fn sync_dir(path: &Path) -> std::io::Result<()> {
    super::sync_store_directory(path)
}

fn ensure_directory_durable(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        return Ok(());
    }
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("closure database path has no parent"))?;
    ensure_directory_durable(parent)?;
    match fs::create_dir(path) {
        Ok(()) => {
            sync_dir(path)?;
            sync_dir(parent)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && path.is_dir() => Ok(()),
        Err(error) => Err(error),
    }
}

fn hex(value: &str) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(value.len() * 2);
    for byte in value.as_bytes() {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

fn valid_receipt_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256-") else {
        return false;
    };
    hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn unhex(value: &str) -> Result<String, String> {
    if !value.len().is_multiple_of(2) {
        return Err("odd hex field".to_string());
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks_exact(2) {
        let high = nibble(pair[0])?;
        let low = nibble(pair[1])?;
        bytes.push((high << 4) | low);
    }
    String::from_utf8(bytes).map_err(|_| "journal field is not UTF-8".to_string())
}

fn nibble(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err("invalid hex field".to_string()),
    }
}

#[cfg(test)]
mod integrity_tests {
    use super::*;

    fn checked(body: String) -> String {
        let checksum = SHA256::sha256_hex(body.as_bytes());
        format!("{body}checksum\t{checksum}\n")
    }

    fn nix_projection_record(
        id: &str,
        output_name: &str,
        digest: &str,
        reference: &str,
    ) -> ClosureRecord {
        let drv = "/nix/store/canonical-action.drv";
        let output_path = format!("/nix/store/{digest}");
        let producer = ProducerRecord::new(
            "nix",
            drv,
            SHA256::sha256_hex(drv.as_bytes()),
            crate::Comptime::Build::BuildPlanReplay::from_facts(BTreeMap::from([
                ("nix.drv_path".into(), drv.into()),
                ("nix.reference".into(), reference.into()),
                (format!("nix.output.{output_name}"), output_path.clone()),
            ]))
            .unwrap(),
            format!("nix-derivation:{drv}"),
            "policy=test\nplatform=test",
            BTreeMap::from([
                ("nix.drv_path".into(), drv.into()),
                (format!("nix.output.{output_name}"), output_path),
            ]),
        )
        .unwrap()
        .encode();
        ClosureRecord {
            id: id.into(),
            primary: digest.into(),
            action_key: "sha256-same-action".into(),
            outputs: BTreeMap::from([
                ("out".into(), digest.into()),
                (output_name.into(), digest.into()),
            ]),
            references: BTreeSet::new(),
            producer_record: producer,
            package_meta: String::new(),
        }
    }

    #[test]
    fn canonical_action_merges_multi_output_alias_projections_and_rejects_conflicting_bytes() {
        let out = nix_projection_record("alias-out", "out", "sha256-out", "pkg@nixpkgs");
        let dev = nix_projection_record("alias-dev", "dev", "sha256-dev", "pkg.dev@stable");
        let objects = ["sha256-out", "sha256-dev"]
            .into_iter()
            .map(|digest| ClosureObject {
                digest: digest.into(),
                path: format!("/nix/store/{digest}"),
                external: true,
            })
            .collect();
        let mut graph = ClosureGraph::default();
        apply_entry(
            &mut graph,
            JournalEntry {
                kind: JournalKind::Delta,
                objects,
                records: vec![out, dev],
                deleted_records: Vec::new(),
            },
        )
        .unwrap();
        assert_eq!(
            graph.action_outputs("sha256-same-action"),
            BTreeMap::from([
                ("dev".into(), "sha256-dev".into()),
                ("out".into(), "sha256-out".into()),
            ])
        );

        let conflict = nix_projection_record(
            "alias-dev-conflict",
            "dev",
            "sha256-other-dev",
            "other:pkg.dev",
        );
        assert!(reject_action_conflict(&graph, &conflict)
            .unwrap_err()
            .contains("conflicting bytes"));
    }

    #[test]
    fn parser_rejects_duplicate_record_ids_and_output_names() {
        let id = hex("record");
        let primary = hex("sha256-primary");
        let action = hex("sha256-action");
        let duplicate_record = checked(format!(
            "jet-closure-journal-v1\nkind\tdelta\nrecord\t{id}\t{primary}\t{action}\nrecord\t{id}\t{primary}\t{action}\n"
        ));
        assert!(parse_entry(&duplicate_record)
            .unwrap_err()
            .contains("duplicate closure record"));

        let out = hex("out");
        let duplicate_output = checked(format!(
            "jet-closure-journal-v1\nkind\tdelta\nrecord\t{id}\t{primary}\t{action}\noutput\t{id}\t{out}\t{primary}\noutput\t{id}\t{out}\t{primary}\n"
        ));
        assert!(parse_entry(&duplicate_output)
            .unwrap_err()
            .contains("duplicate output"));
    }

    #[test]
    fn parser_requires_exact_checksum_framing() {
        let valid = checked("jet-closure-journal-v1\nkind\tdelta\n".to_string());
        assert!(parse_entry(&valid).is_ok());
        assert!(parse_entry(valid.trim_end())
            .unwrap_err()
            .contains("truncated"));

        let mut trailing = valid.clone();
        trailing.push('\n');
        assert!(parse_entry(&trailing)
            .unwrap_err()
            .contains("invalid checksum frame"));

        let upper = valid
            .rsplit_once("checksum\t")
            .unwrap()
            .1
            .to_ascii_uppercase();
        let body = valid.rsplit_once("checksum\t").unwrap().0;
        assert!(parse_entry(&format!("{body}checksum\t{upper}"))
            .unwrap_err()
            .contains("invalid checksum frame"));
    }

    #[test]
    fn graph_validation_rejects_external_and_relation_inconsistency() {
        let roots = Roots {
            root: std::env::temp_dir()
                .join(format!("jet-closure-integrity-{}", std::process::id())),
            dev_mode: true,
        };
        let digest = "sha256-primary".to_string();
        let object = ClosureObject {
            digest: digest.clone(),
            path: roots
                .hangar_dir()
                .join(OBJECTS_DIR)
                .join(&digest)
                .to_string_lossy()
                .into_owned(),
            external: true,
        };
        let record = ClosureRecord {
            id: "record".to_string(),
            primary: digest.clone(),
            action_key: "sha256-action".to_string(),
            outputs: BTreeMap::from([("out".to_string(), digest.clone())]),
            references: BTreeSet::new(),
            producer_record: String::new(),
            package_meta: String::new(),
        };
        let mut graph = ClosureGraph {
            objects: BTreeMap::from([(digest.clone(), object)]),
            records: BTreeMap::from([(record.id.clone(), record)]),
            deleted_records: BTreeSet::new(),
        };
        assert!(validate_graph(&roots, &graph)
            .unwrap_err()
            .contains("invalid external marker"));

        graph.objects.get_mut(&digest).unwrap().external = false;
        graph.records.get_mut("record").unwrap().primary = "sha256-other".to_string();
        assert!(validate_graph(&roots, &graph)
            .unwrap_err()
            .contains("primary is not its `out` output"));

        graph.records.get_mut("record").unwrap().primary = digest.clone();
        graph
            .records
            .get_mut("record")
            .unwrap()
            .references
            .insert("sha256-missing".to_string());
        assert!(validate_graph(&roots, &graph)
            .unwrap_err()
            .contains("references missing object"));
    }
}
