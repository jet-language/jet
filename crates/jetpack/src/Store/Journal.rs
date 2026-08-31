use super::Closure::{
    recover_closure_journal_unlocked, validate_universe_isolation, CanonicalActionRecord,
    ClosureGraph, ClosureObject, ClosureRecord, RECEIPTS_DIR,
};
use super::Receipt::render_receipt;
use super::*;

const JOURNAL_DIR: &str = "journal";
const COMPACT_AFTER: usize = 64;
/// Compact on size as well as count: replay cost is bytes parsed, not file
/// count. A 59-transaction journal reached 57 MiB and made every graph load
/// re-parse and re-hash all of it (Nix avoids this class entirely by keeping
/// current state in one indexed database; our snapshot is the analog).
const COMPACT_AFTER_BYTES: u64 = 4 * 1024 * 1024;
const MAX_CLOSURE_OBJECTS: usize = 1_000_000;
const MAX_CLOSURE_RECORDS: usize = 1_000_000;
const MAX_CLOSURE_DELETIONS: usize = 1_000_000;
const MAX_CLOSURE_TRANSACTIONS: usize = 100_000;
pub(super) const DB_DIR: &str = "closure-db";
pub(super) const PARTIAL_SUFFIX: &str = ".partial";
pub(super) const TXN_SUFFIX: &str = ".txn";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum JournalKind {
    Delta,
    Snapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct JournalEntry {
    pub(super) kind: JournalKind,
    pub(super) objects: Vec<ClosureObject>,
    pub(super) records: Vec<ClosureRecord>,
    pub(super) deleted_records: Vec<String>,
}

pub(super) fn load_graph(roots: &Roots) -> std::io::Result<ClosureGraph> {
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

/// Process cache of the cleanup/lifecycle inputs keyed by the WAL identity
/// stamp. `auto_clean` recomputed this several times per command; each pass
/// replayed the journal with proofs and re-hashed every transaction byte for
/// the closure head.
static LIFECYCLE_INPUTS_CACHE: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<PathBuf, (String, (BTreeSet<String>, String))>>,
> = std::sync::LazyLock::new(Default::default);

pub(super) fn lifecycle_inputs_unlocked(
    roots: &Roots,
) -> std::io::Result<(BTreeSet<String>, String)> {
    let stamp = super::Closure::wal_state_stamp(roots)?;
    if let Ok(cache) = LIFECYCLE_INPUTS_CACHE.lock() {
        if let Some((cached_stamp, inputs)) = cache.get(&roots.root) {
            if *cached_stamp == stamp {
                return Ok(inputs.clone());
            }
        }
    }
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
    let inputs = (
        graph.objects.into_keys().collect::<BTreeSet<String>>(),
        format!("sha256-{}", SHA256::sha256_hex(&canonical)),
    );
    // Recovery above may have rewritten WAL state; stamp the result so the
    // next caller in this command hits.
    let stamp = super::Closure::wal_state_stamp(roots)?;
    if let Ok(mut cache) = LIFECYCLE_INPUTS_CACHE.lock() {
        cache.insert(roots.root.clone(), (stamp, inputs.clone()));
    }
    Ok(inputs)
}

pub(super) fn load_graph_mode(roots: &Roots, allow_legacy: bool) -> std::io::Result<ClosureGraph> {
    load_graph_mode_with_proofs(roots, allow_legacy, true)
}

pub(super) fn load_graph_structure_mode(
    roots: &Roots,
    allow_legacy: bool,
) -> std::io::Result<ClosureGraph> {
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

pub(super) fn apply_entry(graph: &mut ClosureGraph, entry: JournalEntry) -> Result<(), String> {
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
    let mut actions = BTreeMap::new();
    for record in graph.records.values() {
        merge_action_record(&mut actions, record)?;
    }
    for record in entry.records {
        merge_action_record(&mut actions, &record)?;
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

pub(super) fn validate_graph_structure_mode(
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
    let mut actions = BTreeMap::new();
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
        merge_action_record(&mut actions, record)?;
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

pub(super) fn validate_graph_store_proofs(
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

pub(super) fn validate_record_store_proof(
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

fn merge_action_record(
    actions: &mut BTreeMap<String, CanonicalActionRecord>,
    record: &ClosureRecord,
) -> Result<(), String> {
    let projection = canonical_action_projection(record)?;
    if let Some(action) = actions.get_mut(&record.action_key) {
        merge_action_projection(action, &projection, &record.action_key)?;
    } else {
        actions.insert(record.action_key.clone(), projection);
    }
    Ok(())
}

pub(super) fn canonical_action_projection(
    record: &ClosureRecord,
) -> Result<CanonicalActionRecord, String> {
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
    Ingest::try_entry_output_hash(roots, &store_entry_from_meta(&record.id, meta))
        .is_ok_and(|actual| actual == record.primary)
}

pub(super) fn store_entry_from_meta(id: &str, meta: &ParsedMeta) -> StoreEntry {
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

pub(super) fn append_entry(roots: &Roots, entry: &JournalEntry) -> std::io::Result<PathBuf> {
    let journal = journal_dir(roots);
    ensure_directory_durable(&journal)?;
    let sequence = next_sequence(&journal)?;
    write_entry(&journal, sequence, entry)
}

pub(super) fn compact_if_needed(roots: &Roots) -> std::io::Result<()> {
    let journal = journal_dir(roots);
    let mut paths = transaction_paths(&journal)?;
    if paths.len() <= COMPACT_AFTER {
        let total_bytes: u64 = paths
            .iter()
            .filter_map(|path| fs::metadata(path).ok())
            .map(|metadata| metadata.len())
            .sum();
        if paths.len() <= 1 || total_bytes <= COMPACT_AFTER_BYTES {
            return Ok(());
        }
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

pub(super) fn materialize_package_record(
    roots: &Roots,
    record: &ClosureRecord,
) -> std::io::Result<bool> {
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

pub(super) fn remove_package_record(roots: &Roots, id: &str) -> std::io::Result<bool> {
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

pub(super) fn parse_entry(raw: &str) -> Result<JournalEntry, String> {
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

pub(super) fn transaction_paths(journal: &Path) -> std::io::Result<Vec<PathBuf>> {
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

pub(super) fn journal_dir(roots: &Roots) -> PathBuf {
    roots.hangar_dir().join(DB_DIR).join(JOURNAL_DIR)
}

pub(super) fn sync_dir(path: &Path) -> std::io::Result<()> {
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

pub(super) fn hex(value: &str) -> String {
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
        let mut actions = BTreeMap::new();
        for record in graph.records.values() {
            merge_action_record(&mut actions, record).unwrap();
        }
        assert!(merge_action_record(&mut actions, &conflict)
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
