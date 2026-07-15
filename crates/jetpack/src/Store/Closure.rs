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

    pub fn referrers(&self, digest: &str) -> Vec<String> {
        self.records
            .values()
            .filter(|record| record.references.contains(digest))
            .flat_map(|record| record.outputs.values().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
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
    recover_closure_journal(roots)?;
    load_graph(roots)
}

pub fn direct_references_of(roots: &Roots, digest: &str) -> std::io::Result<Vec<String>> {
    Ok(closure_graph(roots)?.direct_references(digest))
}

pub fn transitive_references_of(roots: &Roots, digest: &str) -> std::io::Result<Vec<String>> {
    Ok(closure_graph(roots)?.transitive_references(digest))
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
    let canonical = format!(
        "jet.action-store.v1\nreference={}\nsource={}\nrecipe={}\npolicy={}\nplatform={}\n",
        entry.reference,
        identity.source_fingerprint,
        identity.recipe_fingerprint,
        identity.policy_fingerprint,
        identity.platform,
    );
    format!("sha256-{}", SHA256::sha256_hex(canonical.as_bytes()))
}

pub fn migrate_closure_graph(roots: &Roots) -> std::io::Result<usize> {
    super::super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        migrate_closure_graph_unlocked(roots)
    })
}

fn migrate_closure_graph_unlocked(roots: &Roots) -> std::io::Result<usize> {
    recover_closure_journal(roots)?;
    let mut migrated = 0;
    for entry in list(roots) {
        if register_entry_unlocked(roots, &entry)? {
            migrated += 1;
        }
    }
    Ok(migrated)
}

pub(crate) fn register_entry_unlocked(
    roots: &Roots,
    entry: &StoreEntry,
) -> std::io::Result<bool> {
    if entry.envelope.output_hash.is_empty() {
        return Ok(false);
    }
    recover_closure_journal(roots)?;
    let graph = load_graph(roots)?;
    let (objects, record) = descriptor_for_entry(roots, entry)?;
    if graph.records.get(&record.id) == Some(&record)
        && objects
            .iter()
            .all(|object| graph.objects.get(&object.digest) == Some(object))
    {
        return Ok(false);
    }
    for object in &objects {
        if let Some(existing) = graph.objects.get(&object.digest) {
            if existing != object {
                return Err(std::io::Error::other(format!(
                    "immutable closure object `{}` changed descriptor",
                    object.digest
                )));
            }
        }
    }
    let new_digests = objects
        .iter()
        .map(|object| object.digest.as_str())
        .collect::<BTreeSet<_>>();
    if let Some(missing) = record
        .references
        .iter()
        .find(|digest| !graph.objects.contains_key(*digest) && !new_digests.contains(digest.as_str()))
    {
        return Err(std::io::Error::other(format!(
            "closure record `{}` references missing object `{missing}`",
            record.id
        )));
    }
    append_entry(
        roots,
        &JournalEntry {
            kind: JournalKind::Delta,
            objects,
            records: vec![record],
            deleted_records: Vec::new(),
        },
    )?;
    compact_if_needed(roots)?;
    Ok(true)
}

pub fn remove_closure_record(roots: &Roots, id: &str) -> std::io::Result<bool> {
    super::super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        recover_closure_journal(roots)?;
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
    let mut outputs = entry.named_outputs.clone();
    outputs.insert("out".to_string(), primary.clone());
    let mut objects = Vec::new();
    for (name, digest) in &outputs {
        let path = if name == "out" {
            PathBuf::from(&entry.out)
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
    Ok((
        objects,
        ClosureRecord {
            id: entry.id.clone(),
            primary,
            action_key: entry_action_key(entry),
            outputs,
            references: entry.references.iter().cloned().collect(),
        },
    ))
}

fn load_graph(roots: &Roots) -> std::io::Result<ClosureGraph> {
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
        if entry.kind == JournalKind::Snapshot {
            graph = ClosureGraph::default();
        }
        for id in entry.deleted_records {
            graph.records.remove(&id);
        }
        for object in entry.objects {
            if let Some(existing) = graph.objects.get(&object.digest) {
                if existing != &object {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("closure object `{}` changed immutable descriptor", object.digest),
                    ));
                }
            } else {
                graph.objects.insert(object.digest.clone(), object);
            }
        }
        for record in entry.records {
            graph.records.insert(record.id.clone(), record);
        }
    }
    Ok(graph)
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
            "record\t{}\t{}\t{}\n",
            hex(&record.id),
            hex(&record.primary),
            hex(&record.action_key),
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
            ["object", digest, path, external] => objects.push(ClosureObject {
                digest: unhex(digest)?,
                path: unhex(path)?,
                external: *external == "1",
            }),
            ["record", id, primary, action_key] => {
                let id = unhex(id)?;
                records.insert(
                    id.clone(),
                    ClosureRecord {
                        id,
                        primary: unhex(primary)?,
                        action_key: unhex(action_key)?,
                        outputs: BTreeMap::new(),
                        references: BTreeSet::new(),
                    },
                );
            }
            ["output", id, name, digest] => {
                let id = unhex(id)?;
                let record = records
                    .get_mut(&id)
                    .ok_or_else(|| format!("output precedes record `{id}`"))?;
                record.outputs.insert(unhex(name)?, unhex(digest)?);
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
