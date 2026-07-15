use super::*;

const DB_DIR: &str = "closure-db";
const JOURNAL_DIR: &str = "journal";
const PARTIAL_SUFFIX: &str = ".partial";
const TXN_SUFFIX: &str = ".txn";
const COMPACT_AFTER: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosureObject {
    pub digest: String,
    pub path: String,
    pub external: bool,
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
}

impl ClosureGraph {
    pub fn direct_references(&self, digest: &str) -> Vec<String> {
        let mut out = BTreeSet::new();
        // IngestRequest references describe the whole realization today, so
        // every independently stored named output carries the same closure.
        for record in self
            .records
            .values()
            .filter(|record| record.outputs.values().any(|output| output == digest))
        {
            out.extend(record.references.iter().cloned());
        }
        out.into_iter().collect()
    }

    pub fn transitive_references(&self, digest: &str) -> Vec<String> {
        let mut seen = BTreeSet::new();
        let mut pending = self.direct_references(digest);
        while let Some(next) = pending.pop() {
            if next == digest || !seen.insert(next.clone()) {
                continue;
            }
            pending.extend(self.direct_references(&next));
        }
        seen.into_iter().collect()
    }

    pub fn closure(&self, digest: &str) -> Vec<String> {
        let mut closure = self.transitive_references(digest);
        closure.push(digest.to_string());
        closure.sort();
        closure.dedup();
        closure
    }

    pub fn referrers(&self, digest: &str) -> Vec<String> {
        self.records
            .values()
            .filter(|record| record.references.contains(digest))
            .flat_map(|record| record.outputs.values().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    pub fn transitive_referrers(&self, digest: &str) -> Vec<String> {
        let mut seen = BTreeSet::new();
        let mut pending = self.referrers(digest);
        while let Some(next) = pending.pop() {
            if next == digest || !seen.insert(next.clone()) {
                continue;
            }
            pending.extend(self.referrers(&next));
        }
        seen.into_iter().collect()
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
        for record in self.records.values().filter(|record| record.action_key == action_key) {
            for (name, digest) in &record.outputs {
                out.insert(name.clone(), digest.clone());
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
        recover_closure_journal_unlocked(roots)?;
        migrate_closure_graph_unlocked(roots)?;
        let graph = load_graph(roots)?;
        for record in graph.records.values() {
            materialize_package_record(roots, record)?;
        }
        Ok(graph)
    })
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

pub fn actions_for_output(
    roots: &Roots,
    digest: &str,
) -> std::io::Result<Vec<String>> {
    Ok(closure_graph(roots)?.actions_for_output(digest))
}

pub fn entry_action_key(entry: &StoreEntry) -> String {
    let identity = &entry.cache_identity;
    let mut canonical = b"jet.action-store.v3\0".to_vec();
    for field in [
        entry.reference.as_bytes(),
        identity.source_fingerprint.as_bytes(),
        identity.recipe_fingerprint.as_bytes(),
        identity.policy_fingerprint.as_bytes(),
        identity.platform.as_bytes(),
        entry.producer_record.as_bytes(),
    ] {
        push_frame(&mut canonical, field);
    }
    let references = entry.references.iter().collect::<BTreeSet<_>>();
    canonical.extend_from_slice(&(references.len() as u64).to_be_bytes());
    for digest in references {
        push_frame(&mut canonical, digest.as_bytes());
    }
    format!("sha256-{}", SHA256::sha256_hex(&canonical))
}

fn push_frame(out: &mut Vec<u8>, field: &[u8]) {
    out.extend_from_slice(&(field.len() as u64).to_be_bytes());
    out.extend_from_slice(field);
}

pub fn migrate_closure_graph(roots: &Roots) -> std::io::Result<usize> {
    super::super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        migrate_closure_graph_unlocked(roots)
    })
}

fn migrate_closure_graph_unlocked(roots: &Roots) -> std::io::Result<usize> {
    recover_closure_journal_unlocked(roots)?;
    let mut graph = load_graph_mode(roots, true)?;
    let mut entries = list(roots);
    entries.sort_by(|left, right| left.id.cmp(&right.id));
    let mut seen_records = BTreeSet::new();
    let mut objects = BTreeMap::new();
    let mut records = Vec::new();
    for entry in entries {
        let entry = normalize_legacy_entry(entry)?;
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
        return Ok(0);
    }
    let migrated = records.len();
    let transaction = JournalEntry {
        kind: JournalKind::Delta,
        objects: objects.into_values().collect(),
        records,
        deleted_records: Vec::new(),
    };
    apply_entry(&mut graph, transaction.clone()).map_err(std::io::Error::other)?;
    validate_graph(roots, &graph).map_err(std::io::Error::other)?;
    append_entry(roots, &transaction)?;
    for record in &transaction.records {
        materialize_package_record(roots, record)?;
    }
    compact_if_needed(roots)?;
    Ok(migrated)
}

pub(crate) fn register_entry_unlocked(
    roots: &Roots,
    entry: &StoreEntry,
) -> std::io::Result<bool> {
    if entry.envelope.output_hash.is_empty() {
        return Ok(false);
    }
    recover_closure_journal_unlocked(roots)?;
    migrate_closure_graph_unlocked(roots)?;
    let graph = load_graph(roots)?;
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
    validate_graph(roots, &candidate).map_err(std::io::Error::other)?;
    append_entry(roots, &transaction)?;
    materialize_package_record(roots, &transaction.records[0])?;
    compact_if_needed(roots)?;
    Ok(true)
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
        compact_if_needed(roots)?;
        Ok(true)
    })
}

pub fn recover_closure_journal(roots: &Roots) -> std::io::Result<usize> {
    super::super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        recover_closure_journal_unlocked(roots)
    })
}

fn recover_closure_journal_unlocked(roots: &Roots) -> std::io::Result<usize> {
    let journal = journal_dir(roots);
    let Ok(entries) = fs::read_dir(&journal) else {
        return Ok(0);
    };
    let mut recovered = 0;
    for entry in entries.flatten() {
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
    Ok(recovered)
}

fn descriptor_for_entry(
    roots: &Roots,
    entry: &StoreEntry,
) -> std::io::Result<(Vec<ClosureObject>, ClosureRecord)> {
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
    let mut objects = Vec::new();
    for (name, digest) in &outputs {
        let path = if name == "out" {
            PathBuf::from(&entry.out)
        } else if let Ok(producer) = ProducerRecord::decode(&entry.producer_record) {
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
    entry.producer_record = canonical_producer(
        "legacy-migration",
        &format!("cas:{}", identity.source_fingerprint),
        &identity.source_fingerprint,
        identity,
        BTreeMap::from([("legacy.reference".into(), entry.reference.clone())]),
    )?;
    Ok(entry)
}

fn load_graph(roots: &Roots) -> std::io::Result<ClosureGraph> {
    load_graph_mode(roots, false)
}

fn load_graph_mode(roots: &Roots, allow_legacy: bool) -> std::io::Result<ClosureGraph> {
    let journal = journal_dir(roots);
    let Ok(entries) = fs::read_dir(&journal) else {
        return Ok(ClosureGraph::default());
    };
    let mut paths = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("txn"))
        .collect::<Vec<_>>();
    paths.sort();
    let mut graph = ClosureGraph::default();
    for path in paths {
        let entry = parse_entry(&fs::read_to_string(&path)?).map_err(|error| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("closure journal `{}`: {error}", path.display()),
            )
        })?;
        apply_entry(&mut graph, entry)
            .and_then(|()| validate_graph_mode(roots, &graph, allow_legacy))
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    }
    Ok(graph)
}

fn apply_entry(graph: &mut ClosureGraph, entry: JournalEntry) -> Result<(), String> {
    if entry.kind == JournalKind::Snapshot {
        *graph = ClosureGraph::default();
    }
    for id in entry.deleted_records {
        graph.records.remove(&id);
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
        graph.records.insert(record.id.clone(), record);
    }
    Ok(())
}

fn validate_graph(roots: &Roots, graph: &ClosureGraph) -> Result<(), String> {
    validate_graph_mode(roots, graph, false)
}

fn validate_graph_mode(
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
    let mut actions: BTreeMap<&str, &ClosureRecord> = BTreeMap::new();
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
                return Err(format!("closure record `{id}` has invalid output name `{name}`"));
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
        if let Some(existing) = actions.insert(&record.action_key, record) {
            if existing.outputs != record.outputs
                || existing.references != record.references
                || existing.producer_record != record.producer_record
            {
                return Err(format!(
                    "action `{}` maps to conflicting records `{}` and `{}`",
                    record.action_key, existing.id, record.id
                ));
            }
        }
    }
    Ok(())
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
    if let Some(existing) = graph.records.values().find(|record| {
        record.action_key == candidate.action_key
            && (record.outputs != candidate.outputs
                || record.references != candidate.references
                || record.producer_record != candidate.producer_record)
    }) {
        return Err(format!(
            "action `{}` maps to conflicting records `{}` and `{}`",
            candidate.action_key, existing.id, candidate.id
        ));
    }
    Ok(())
}

fn append_entry(roots: &Roots, entry: &JournalEntry) -> std::io::Result<()> {
    let journal = journal_dir(roots);
    fs::create_dir_all(&journal)?;
    let sequence = next_sequence(&journal)?;
    write_entry(&journal, sequence, entry)
}

fn compact_if_needed(roots: &Roots) -> std::io::Result<()> {
    let journal = journal_dir(roots);
    let mut paths = transaction_paths(&journal)?;
    if paths.len() <= COMPACT_AFTER {
        return Ok(());
    }
    let graph = load_graph(roots)?;
    let snapshot = JournalEntry {
        kind: JournalKind::Snapshot,
        objects: graph.objects.into_values().collect(),
        records: graph.records.into_values().collect(),
        deleted_records: Vec::new(),
    };
    let sequence = next_sequence(&journal)?;
    write_entry(&journal, sequence, &snapshot)?;
    paths.sort();
    for path in paths {
        fs::remove_file(path)?;
    }
    sync_dir(&journal)
}

fn write_entry(journal: &Path, sequence: u64, entry: &JournalEntry) -> std::io::Result<()> {
    let text = render_entry(entry);
    let checksum = SHA256::sha256_hex(text.as_bytes());
    let final_path = journal.join(format!("{sequence:020}-{}.txn", &checksum[..16]));
    let partial = journal.join(format!("{sequence:020}-{}.partial", &checksum[..16]));
    fs::write(&partial, format!("{text}checksum\t{checksum}\n"))?;
    fs::File::open(&partial)?.sync_all()?;
    fs::rename(&partial, &final_path)?;
    sync_dir(journal)
}

fn materialize_package_record(roots: &Roots, record: &ClosureRecord) -> std::io::Result<()> {
    let dir = roots.hangar_dir().join(&record.id);
    fs::create_dir_all(&dir)?;
    let path = dir.join("meta.json");
    if fs::read_to_string(&path).ok().as_deref() == Some(record.package_meta.as_str()) {
        return Ok(());
    }
    let tmp = dir.join(format!("meta.json.{}.partial", std::process::id()));
    fs::write(&tmp, &record.package_meta)?;
    fs::File::open(&tmp)?.sync_all()?;
    fs::rename(&tmp, &path)?;
    sync_dir(&dir)
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
    let checksum = checksum_line.trim();
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
            ["object", digest, path, external @ ("0" | "1")] => objects.push(ClosureObject {
                digest: unhex(digest)?,
                path: unhex(path)?,
                external: *external == "1",
            }),
            ["record", id, primary, action_key, producer_record, package_meta] => {
                let id = unhex(id)?;
                if records.contains_key(&id) {
                    return Err(format!("duplicate closure record `{id}`"));
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
            ["delete", id] => deleted_records.push(unhex(id)?),
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
    Ok(entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("txn"))
        .collect())
}

fn journal_dir(roots: &Roots) -> PathBuf {
    roots.hangar_dir().join(DB_DIR).join(JOURNAL_DIR)
}

fn sync_dir(path: &Path) -> std::io::Result<()> {
    fs::File::open(path)?.sync_all()
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
    fn graph_validation_rejects_external_and_relation_inconsistency() {
        let roots = Roots {
            root: std::env::temp_dir().join(format!(
                "jet-closure-integrity-{}",
                std::process::id()
            )),
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
        graph.records
            .get_mut("record")
            .unwrap()
            .references
            .insert("sha256-missing".to_string());
        assert!(validate_graph(&roots, &graph)
            .unwrap_err()
            .contains("references missing object"));
    }
}
