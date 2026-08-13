// D-SYNC1=A / D-DBPOLICY1=A (#1159/#1160): CRDT values + typed row policies.

use super::{__jet_Decode, __jet_Encode, JetShow};
use super::jet_std;

pub(crate) const MAX_SYNC_TEXT: usize = 1024 * 1024;
/// Tombstones outlive the characters they replace, so a document holds more
/// atoms than it shows.  This bounds the whole atom set, checked before a
/// write rather than by discarding atoms during a merge.
pub(crate) const MAX_SYNC_ATOMS: usize = 4 * 1024 * 1024;
pub(crate) const MAX_SYNC_REPLICAS: usize = 4096;
pub(crate) const MAX_SYNC_ENTRIES: usize = 100_000;
pub(crate) const MAX_SYNC_SESSION: usize = 256;
pub(crate) const MAX_SYNC_DOCUMENT: usize = 4 * 1024 * 1024;

/// D-SYNC1: SyncText is a sequence CRDT, not a copy per replica.  Every
/// character is one atom carrying a unique `(replica, counter)` identity and
/// the identity of the atom it follows.  Reading order is recomputed from
/// those identities, so a merge is the union of two atom sets with deletions
/// winning: commutative, associative, and idempotent.  Two replicas that edit
/// while apart converge on one document, and neither loses characters.
#[derive(Clone, Debug)]
pub struct JetSyncText {
    pub atoms: Vec<JetSyncTextAtom>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetSyncTextAtom {
    pub replica: String,
    pub counter: u64,
    /// The atom this one follows.  `None` places it at the document start.
    pub after: Option<(String, u64)>,
    pub ch: char,
    pub deleted: bool,
}

#[derive(Clone, Debug)]
pub struct JetSyncCounter {
    pub counts: Vec<(String, u64, u64)>, // replica_id, positive, negative
}

#[derive(Clone, Debug)]
pub struct JetSyncMap {
    pub entries: Vec<(String, String, u64, String)>, // key, value, clock, writer
}

/// D-SYNC1: the typed map carrier.  The legacy `JetSyncMap` above remains the
/// String/String compatibility spelling used by the original fixed Core API.
/// Generic maps keep their actual Codable key and value instead of erasing
/// them to text at the runtime boundary.
#[derive(Clone, Debug)]
pub struct JetSyncMapGeneric<K, V> {
    pub entries: Vec<(K, V, u64, String)>, // key, value, logical clock, writer
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum JetRowPolicyExpr {
    AllowAll,
    OwnerEqualsUser,
}

#[derive(Clone, Debug)]
pub struct JetRowPolicy {
    pub table: String,
    pub expression: String,
    compiled: JetRowPolicyExpr,
}

/// D-DBPOLICY-BIND1: policy authority is a distinct capability. Keeping the
/// policy and user on the scope prevents a caller from replacing either one
/// between SQL operations while retaining the underlying connection handle.
#[derive(Clone, Debug)]
pub struct JetDbScope {
    pub handle: u64,
    pub policy: JetRowPolicy,
    pub user: String,
}

#[derive(Clone, Debug)]
pub struct JetSyncList {
    pub items: Vec<(String, String)>, // replica_id, serialized item
}

/// D-SYNC1: the typed list carrier.  List membership is an add-only CRDT;
/// identity is the replica plus the canonical Codable item value.
#[derive(Clone, Debug)]
pub struct JetSyncListGeneric<T> {
    pub items: Vec<(String, T)>, // replica_id, item
}

/// Reading order (RGA).  Atoms sharing an anchor are ordered by descending
/// counter, then descending replica, so concurrent inserts at one point land
/// in the same order on every replica.  An atom whose anchor is absent is
/// unreachable and stays out of the order rather than moving to the start.
pub(crate) fn jet_sync_text_order(doc: &JetSyncText) -> Vec<usize> {
    let mut children: std::collections::BTreeMap<Option<(String, u64)>, Vec<usize>> =
        std::collections::BTreeMap::new();
    for (index, atom) in doc.atoms.iter().enumerate() {
        children.entry(atom.after.clone()).or_default().push(index);
    }
    for kids in children.values_mut() {
        kids.sort_by(|left, right| {
            let left = &doc.atoms[*left];
            let right = &doc.atoms[*right];
            right
                .counter
                .cmp(&left.counter)
                .then_with(|| right.replica.cmp(&left.replica))
        });
    }
    let mut order = Vec::with_capacity(doc.atoms.len());
    let mut stack: Vec<usize> = Vec::new();
    if let Some(roots) = children.get(&None) {
        stack.extend(roots.iter().rev().copied());
    }
    while let Some(index) = stack.pop() {
        order.push(index);
        let id = Some((doc.atoms[index].replica.clone(), doc.atoms[index].counter));
        if let Some(kids) = children.get(&id) {
            stack.extend(kids.iter().rev().copied());
        }
    }
    order
}

fn jet_sync_text_visible(doc: &JetSyncText) -> Vec<usize> {
    jet_sync_text_order(doc)
        .into_iter()
        .filter(|index| !doc.atoms[*index].deleted)
        .collect()
}

fn jet_sync_text_read(doc: &JetSyncText) -> String {
    jet_sync_text_visible(doc)
        .into_iter()
        .map(|index| doc.atoms[index].ch)
        .collect()
}

/// The counter is a document-wide Lamport clock, not a per-replica one.  The
/// sibling rule above compares counters across replicas, so a per-replica
/// counter would place a new character by how much its own replica had written
/// rather than by what it was inserted after.
fn jet_sync_text_next_counter(doc: &JetSyncText) -> u64 {
    doc.atoms
        .iter()
        .map(|atom| atom.counter)
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

/// Would writing `count` more characters run the clock out?  Saturating here
/// would mint an identity a document already holds, so callers refuse instead.
fn jet_sync_text_clock_exhausted(doc: &JetSyncText, count: usize) -> bool {
    u64::try_from(count)
        .ok()
        .and_then(|count| jet_sync_text_next_counter(doc).checked_add(count))
        .is_none()
}

/// Write `text` as a chain of atoms following `anchor`.  Each character keeps
/// its own identity, which is what lets a later insert land between two
/// existing characters.
fn jet_sync_text_insert_after(
    doc: &mut JetSyncText,
    replica: &str,
    anchor: Option<(String, u64)>,
    text: &str,
) {
    let mut anchor = anchor;
    let mut counter = jet_sync_text_next_counter(doc);
    for ch in text.chars() {
        doc.atoms.push(JetSyncTextAtom {
            replica: replica.to_string(),
            counter,
            after: anchor,
            ch,
            deleted: false,
        });
        anchor = Some((replica.to_string(), counter));
        counter += 1;
    }
}

pub(crate) fn jet_sync_text_new(replica: String, text: String) -> JetSyncText {
    let mut doc = JetSyncText { atoms: Vec::new() };
    if !jet_sync_token_is_valid(&replica) || text.chars().count() > MAX_SYNC_TEXT {
        return doc;
    }
    jet_sync_text_insert_after(&mut doc, &replica, None, &text);
    doc
}

/// Replace the whole document: every visible atom becomes a tombstone and the
/// new text is written at the start.  The tombstones stay so a replica that
/// never saw them still converges here after a merge.
pub(crate) fn jet_sync_text_set(mut doc: JetSyncText, replica: String, text: String) -> JetSyncText {
    let inserted = text.chars().count();
    if !jet_sync_token_is_valid(&replica)
        || inserted > MAX_SYNC_TEXT
        || doc.atoms.len().saturating_add(inserted) > MAX_SYNC_ATOMS
        || jet_sync_text_clock_exhausted(&doc, inserted)
    {
        return doc;
    }
    for index in jet_sync_text_visible(&doc) {
        doc.atoms[index].deleted = true;
    }
    jet_sync_text_insert_after(&mut doc, &replica, None, &text);
    doc
}

/// Apply one edit at a visible-character position.  Indices are Unicode scalar
/// positions, never byte offsets.  The insert anchors to the character before
/// the replaced range, so it survives a concurrent edit to that range.
pub(crate) fn jet_sync_text_edit(
    mut doc: JetSyncText,
    replica: String,
    index: i64,
    delete_count: i64,
    insert: String,
) -> JetSyncText {
    let inserted = insert.chars().count();
    if !jet_sync_token_is_valid(&replica)
        || index < 0
        || delete_count < 0
        || inserted > MAX_SYNC_TEXT
        || doc.atoms.len().saturating_add(inserted) > MAX_SYNC_ATOMS
        || jet_sync_text_clock_exhausted(&doc, inserted)
    {
        return doc;
    }
    let (Ok(index), Ok(delete_count)) = (usize::try_from(index), usize::try_from(delete_count))
    else {
        return doc;
    };
    let visible = jet_sync_text_visible(&doc);
    if index > visible.len() || delete_count > visible.len().saturating_sub(index) {
        return doc;
    }
    let anchor = index.checked_sub(1).map(|before| {
        let atom = &doc.atoms[visible[before]];
        (atom.replica.clone(), atom.counter)
    });
    for offset in 0..delete_count {
        doc.atoms[visible[index + offset]].deleted = true;
    }
    jet_sync_text_insert_after(&mut doc, &replica, anchor, &insert);
    doc
}

/// Union the two atom sets.  Nothing is dropped: a merge that discarded atoms
/// to stay under a limit would lose a replica's writing.  A deletion is
/// monotone, so once either side has seen it the merged atom stays deleted.
pub(crate) fn jet_sync_text_merge(a: &JetSyncText, b: &JetSyncText) -> JetSyncText {
    let mut merged: std::collections::BTreeMap<(String, u64), JetSyncTextAtom> =
        std::collections::BTreeMap::new();
    for atom in a.atoms.iter().chain(&b.atoms) {
        if !jet_sync_token_is_valid(&atom.replica) {
            continue;
        }
        let key = (atom.replica.clone(), atom.counter);
        match merged.get_mut(&key) {
            Some(existing) => {
                let deleted = existing.deleted || atom.deleted;
                // One identity carrying two readings means a replica name was
                // used by two writers, or one document was forked and both
                // halves edited under the same name.  An anchor must resolve
                // to one parent, so only one reading can survive; keep the
                // smaller so the merge answers the same either way round.
                // A replica name has to own a single line of edits.
                if (atom.ch, &atom.after) < (existing.ch, &existing.after) {
                    existing.ch = atom.ch;
                    existing.after = atom.after.clone();
                }
                existing.deleted = deleted;
            }
            None => {
                merged.insert(key, atom.clone());
            }
        }
    }
    JetSyncText {
        atoms: merged.into_values().collect(),
    }
}

pub(crate) fn jet_sync_text_show(doc: &JetSyncText) -> String {
    format!("SyncText({})", jet_sync_text_read(doc))
}

/// The highest counter each replica has written, plus the document clock.
/// This is not a vector clock: a Lamport counter records when a replica wrote,
/// never what it had seen, so it orders edits but cannot decide causality.
pub(crate) fn jet_sync_text_metadata(doc: &JetSyncText) -> String {
    let mut clocks: std::collections::BTreeMap<&str, u64> = std::collections::BTreeMap::new();
    for atom in &doc.atoms {
        let slot = clocks.entry(atom.replica.as_str()).or_insert(0);
        if atom.counter > *slot {
            *slot = atom.counter;
        }
    }
    let parts = clocks
        .iter()
        .map(|(replica, clock)| format!("{replica}={clock}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("LamportClock({parts})")
}

pub(crate) fn jet_sync_counter_new(replica: String, value: i64) -> JetSyncCounter {
    if !jet_sync_token_is_valid(&replica) {
        return JetSyncCounter { counts: Vec::new() };
    }
    JetSyncCounter {
        counts: vec![(
            replica,
            if value >= 0 { value as u64 } else { 0 },
            if value < 0 { value.unsigned_abs() } else { 0 },
        )],
    }
}

pub(crate) fn jet_sync_counter_inc(mut counter: JetSyncCounter, replica: String, delta: i64) -> JetSyncCounter {
    if !jet_sync_token_is_valid(&replica) {
        return counter;
    }
    if let Some((_, positive, negative)) = counter
        .counts
        .iter_mut()
        .find(|(r, _, _)| r == &replica)
    {
        if delta >= 0 {
            *positive = positive.saturating_add(delta as u64);
        } else {
            *negative = negative.saturating_add(delta.unsigned_abs());
        }
    } else {
        if counter.counts.len() >= MAX_SYNC_REPLICAS {
            return counter;
        }
        counter.counts.push((
            replica,
            if delta >= 0 { delta as u64 } else { 0 },
            if delta < 0 { delta.unsigned_abs() } else { 0 },
        ));
    }
    counter.counts.sort_by(|left, right| left.0.cmp(&right.0));
    counter
}

pub(crate) fn jet_sync_counter_merge(a: &JetSyncCounter, b: &JetSyncCounter) -> JetSyncCounter {
    let mut merged = std::collections::BTreeMap::<String, (u64, u64)>::new();
    for (replica, positive, negative) in a.counts.iter().chain(&b.counts) {
        if !jet_sync_token_is_valid(replica) {
            continue;
        }
        let entry = merged.entry(replica.clone()).or_insert((0, 0));
        entry.0 = entry.0.max(*positive);
        entry.1 = entry.1.max(*negative);
    }
    JetSyncCounter {
        counts: merged
            .into_iter()
            .take(MAX_SYNC_REPLICAS)
            .map(|(replica, (positive, negative))| (replica, positive, negative))
            .collect(),
    }
}

pub(crate) fn jet_sync_counter_value(counter: &JetSyncCounter) -> i64 {
    counter.counts.iter().fold(0i64, |sum, (_, positive, negative)| {
        let value = if positive >= negative {
            i64::try_from(positive - negative).unwrap_or(i64::MAX)
        } else {
            let magnitude = negative - positive;
            if magnitude > (i64::MAX as u64) + 1 {
                i64::MIN
            } else if magnitude == (i64::MAX as u64) + 1 {
                i64::MIN
            } else {
                -(magnitude as i64)
            }
        };
        sum.saturating_add(value)
    })
}

fn jet_sync_counter_metadata(counter: &JetSyncCounter) -> String {
    let parts = counter
        .counts
        .iter()
        .map(|(replica, positive, negative)| format!("{replica}=+{positive}/-{negative}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("PNCounter({parts})")
}

pub(crate) fn jet_sync_map_new() -> JetSyncMap {
    JetSyncMap {
        entries: Vec::new(),
    }
}

fn jet_sync_map_set_replica(
    mut map: JetSyncMap,
    replica: String,
    key: String,
    value: String,
) -> JetSyncMap {
    if !jet_sync_token_is_valid(&replica)
        || !jet_sync_token_is_valid(&key)
        || value.len() > MAX_SYNC_TEXT
    {
        return map;
    }
    let next_clock = map
        .entries
        .iter()
        .filter(|(_, _, _, writer)| writer == &replica)
        .map(|(_, _, clock, _)| *clock)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    if let Some((_, v, clock, writer)) = map.entries.iter_mut().find(|(k, _, _, _)| k == &key) {
        *v = value;
        *clock = next_clock;
        *writer = replica;
    } else {
        if map.entries.len() >= MAX_SYNC_ENTRIES {
            return map;
        }
        map.entries.push((key, value, next_clock, replica));
    }
    // Local edits use one canonical order too.  Merge already returns a
    // BTreeMap order; keeping the local path identical makes show/serialize
    // deterministic before the first merge and after duplicate delivery.
    map.entries.sort_by(|left, right| left.0.cmp(&right.0));
    map
}

pub(crate) fn jet_sync_map_set(map: JetSyncMap, key: String, value: String) -> JetSyncMap {
    jet_sync_map_set_replica(map, "local".to_string(), key, value)
}

pub(crate) fn jet_sync_map_get(map: &JetSyncMap, key: &String) -> Option<String> {
    map.entries
        .iter()
        .find(|(k, _, _, _)| k == key)
        .map(|(_, v, _, _)| v.clone())
}

pub(crate) fn jet_sync_map_merge(a: &JetSyncMap, b: &JetSyncMap) -> JetSyncMap {
    let mut merged = std::collections::BTreeMap::<String, (String, u64, String)>::new();
    for (key, value, clock, writer) in a.entries.iter().chain(&b.entries) {
        if !jet_sync_token_is_valid(key)
            || !jet_sync_token_is_valid(writer)
            || value.len() > MAX_SYNC_TEXT
        {
            continue;
        }
        let replace = merged.get(key).map_or(true, |(existing, existing_clock, existing_writer)| {
            *clock > *existing_clock
                || (*clock == *existing_clock
                    && (writer.as_str(), value.as_str())
                        > (existing_writer.as_str(), existing.as_str()))
        });
        if replace {
            merged.insert(key.clone(), (value.clone(), *clock, writer.clone()));
        }
    }
    JetSyncMap {
        entries: merged
            .into_iter()
            .take(MAX_SYNC_ENTRIES)
            .map(|(key, (value, clock, writer))| (key, value, clock, writer))
            .collect(),
    }
}

pub(crate) fn jet_sync_map_show(map: &JetSyncMap) -> String {
    let parts = map
        .entries
        .iter()
        .map(|(k, v, _, _)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("SyncMap({parts})")
}

fn jet_sync_map_metadata(map: &JetSyncMap) -> String {
    let parts = map
        .entries
        .iter()
        .map(|(key, _, clock, writer)| format!("{key}@{writer}={clock}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("LWWMap({parts})")
}

fn jet_sync_value_id<T: __jet_Encode>(value: &T) -> String {
    // Canonical DataTree JSON is the wire identity.  It is deterministic for
    // primitive, collection, and user-derived Codable values, so map keys do
    // not need an invented Rust `Ord` requirement.
    jet_std::render_datatree_json(&value.jet_encode(), true, 0)
}

fn jet_sync_value_show<T: __jet_Encode>(value: &T) -> String {
    jet_std::render_datatree_json(&value.jet_encode(), false, 0)
}

fn jet_sync_map_new_generic<K, V>() -> JetSyncMapGeneric<K, V> {
    JetSyncMapGeneric { entries: Vec::new() }
}

fn jet_sync_map_set_generic<K, V>(
    mut map: JetSyncMapGeneric<K, V>,
    replica: String,
    key: K,
    value: V,
) -> JetSyncMapGeneric<K, V>
where
    K: __jet_Encode,
    V: __jet_Encode,
{
    if !jet_sync_token_is_valid(&replica) {
        return map;
    }
    let key_id = jet_sync_value_id(&key);
    let next_clock = map
        .entries
        .iter()
        .filter(|(_, _, _, writer)| writer == &replica)
        .map(|(_, _, clock, _)| *clock)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    if let Some((existing_key, existing_value, clock, writer)) = map
        .entries
        .iter_mut()
        .find(|(existing_key, _, _, _)| jet_sync_value_id(existing_key) == key_id)
    {
        *existing_key = key;
        *existing_value = value;
        *clock = next_clock;
        *writer = replica;
    } else if map.entries.len() < MAX_SYNC_ENTRIES {
        map.entries.push((key, value, next_clock, replica));
    }
    map.entries.sort_by(|left, right| {
        let left_id = jet_sync_value_id(&left.0);
        let right_id = jet_sync_value_id(&right.0);
        (left_id, left.3.as_str(), left.2).cmp(&(right_id, right.3.as_str(), right.2))
    });
    map
}

fn jet_sync_map_get_generic<K, V>(map: &JetSyncMapGeneric<K, V>, key: &K) -> Option<V>
where
    K: __jet_Encode,
    V: Clone,
{
    let key_id = jet_sync_value_id(key);
    map.entries
        .iter()
        .find(|(existing_key, _, _, _)| jet_sync_value_id(existing_key) == key_id)
        .map(|(_, value, _, _)| value.clone())
}

fn jet_sync_map_merge_generic<K, V>(
    a: &JetSyncMapGeneric<K, V>,
    b: &JetSyncMapGeneric<K, V>,
) -> JetSyncMapGeneric<K, V>
where
    K: Clone + __jet_Encode,
    V: Clone + __jet_Encode,
{
    let mut merged = std::collections::BTreeMap::<String, (K, V, u64, String)>::new();
    for (key, value, clock, writer) in a.entries.iter().chain(&b.entries) {
        let key_id = jet_sync_value_id(key);
        let value_id = jet_sync_value_id(value);
        let replace = merged.get(&key_id).map_or(true, |(_, existing, existing_clock, existing_writer)| {
            *clock > *existing_clock
                || (*clock == *existing_clock
                    && (writer.as_str(), value_id.as_str())
                        > (existing_writer.as_str(), jet_sync_value_id(existing).as_str()))
        });
        if replace {
            merged.insert(key_id, (key.clone(), value.clone(), *clock, writer.clone()));
        }
    }
    JetSyncMapGeneric {
        entries: merged
            .into_values()
            .take(MAX_SYNC_ENTRIES)
            .collect(),
    }
}

fn jet_sync_map_show_generic<K, V>(map: &JetSyncMapGeneric<K, V>) -> String
where
    K: __jet_Encode,
    V: __jet_Encode,
{
    let parts = map
        .entries
        .iter()
        .map(|(key, value, _, _)| {
            format!("{}={}", jet_sync_value_show(key), jet_sync_value_show(value))
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("SyncMap({parts})")
}

fn jet_sync_map_metadata_generic<K, V>(map: &JetSyncMapGeneric<K, V>) -> String
where
    K: __jet_Encode,
{
    let parts = map
        .entries
        .iter()
        .map(|(key, _, clock, writer)| {
            format!("{}@{writer}={clock}", jet_sync_value_id(key))
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("LWWMap({parts})")
}

pub(crate) fn jet_db_policy_new(table: String, expression: String) -> Result<JetRowPolicy, String> {
    let table = table.trim().to_string();
    let expression = expression.trim().to_string();
    let valid_table = table
        .chars()
        .enumerate()
        .all(|(index, ch)| {
            (index == 0 && (ch.is_ascii_alphabetic() || ch == '_'))
                || (index > 0 && (ch.is_ascii_alphanumeric() || ch == '_'))
        });
    if table.is_empty()
        || !valid_table
        || expression.is_empty()
        || table.len() > MAX_SYNC_TEXT
        || expression.len() > MAX_SYNC_TEXT
        || table.chars().any(char::is_control)
        || expression.chars().any(char::is_control)
    {
        return Err("row policy needs a table and expression".to_string());
    }
    let compiled = match expression.trim() {
        "true" => JetRowPolicyExpr::AllowAll,
        "owner == user" => JetRowPolicyExpr::OwnerEqualsUser,
        other => {
            return Err(format!(
                "unsupported row policy expression `{other}`; supported forms are `true` and `owner == user`"
            ));
        }
    };
    Ok(JetRowPolicy {
        table,
        expression,
        compiled,
    })
}

pub(crate) fn jet_db_policy_allows(policy: &JetRowPolicy, user: &String, row_owner: &String) -> bool {
    if !jet_sync_token_is_valid(user) || !jet_sync_token_is_valid(row_owner) {
        return false;
    }
    match &policy.compiled {
        JetRowPolicyExpr::OwnerEqualsUser => user == row_owner,
        JetRowPolicyExpr::AllowAll => true,
    }
}

pub(crate) fn jet_db_policy_show(policy: &JetRowPolicy) -> String {
    format!("RowPolicy(table={}, expr={})", policy.table, policy.expression)
}

pub(crate) fn jet_sync_list_new() -> JetSyncList {
    JetSyncList { items: Vec::new() }
}

pub(crate) fn jet_sync_list_push(mut list: JetSyncList, replica: String, item: String) -> JetSyncList {
    if !jet_sync_token_is_valid(&replica) || item.len() > MAX_SYNC_TEXT {
        return list;
    }
    if !list.items.iter().any(|(r, i)| r == &replica && i == &item) {
        if list.items.len() < MAX_SYNC_ENTRIES {
            list.items.push((replica, item));
        }
    }
    list.items.sort_by(|left, right| (&left.0, &left.1).cmp(&(&right.0, &right.1)));
    list
}

pub(crate) fn jet_sync_list_merge(a: &JetSyncList, b: &JetSyncList) -> JetSyncList {
    let mut out = a.clone();
    for (replica, item) in &b.items {
        if !out
            .items
            .iter()
            .any(|(r, i)| r == replica && i == item)
        {
            out.items.push((replica.clone(), item.clone()));
        }
    }
    out.items.retain(|(replica, item)| {
        jet_sync_token_is_valid(replica) && item.len() <= MAX_SYNC_TEXT
    });
    out.items.sort_by(|l, r| (&l.0, &l.1).cmp(&(&r.0, &r.1)));
    out.items.truncate(MAX_SYNC_ENTRIES);
    out
}

pub(crate) fn jet_sync_token_is_valid(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= MAX_SYNC_TEXT
        && !value.chars().any(char::is_control)
}

pub(crate) fn jet_sync_list_show(list: &JetSyncList) -> String {
    let parts = list
        .items
        .iter()
        .map(|(r, i)| format!("{r}:{i}"))
        .collect::<Vec<_>>()
        .join("|");
    format!("SyncList({parts})")
}

fn jet_sync_list_metadata(list: &JetSyncList) -> String {
    format!("GSet(items={})", list.items.len())
}

fn jet_sync_list_new_generic<T>() -> JetSyncListGeneric<T> {
    JetSyncListGeneric { items: Vec::new() }
}

fn jet_sync_list_push_generic<T>(
    mut list: JetSyncListGeneric<T>,
    replica: String,
    item: T,
) -> JetSyncListGeneric<T>
where
    T: __jet_Encode,
{
    if !jet_sync_token_is_valid(&replica) {
        return list;
    }
    let item_id = jet_sync_value_id(&item);
    if !list
        .items
        .iter()
        .any(|(existing_replica, existing_item)| {
            existing_replica == &replica && jet_sync_value_id(existing_item) == item_id
        })
        && list.items.len() < MAX_SYNC_ENTRIES
    {
        list.items.push((replica, item));
    }
    list.items.sort_by(|left, right| {
        let left_id = jet_sync_value_id(&left.1);
        let right_id = jet_sync_value_id(&right.1);
        (left.0.as_str(), left_id).cmp(&(right.0.as_str(), right_id))
    });
    list
}

fn jet_sync_list_merge_generic<T>(
    a: &JetSyncListGeneric<T>,
    b: &JetSyncListGeneric<T>,
) -> JetSyncListGeneric<T>
where
    T: Clone + __jet_Encode,
{
    let mut out = a.clone();
    for (replica, item) in &b.items {
        let item_id = jet_sync_value_id(item);
        if !out.items.iter().any(|(existing_replica, existing_item)| {
            existing_replica == replica && jet_sync_value_id(existing_item) == item_id
        }) && out.items.len() < MAX_SYNC_ENTRIES {
            out.items.push((replica.clone(), item.clone()));
        }
    }
    out.items.sort_by(|left, right| {
        let left_id = jet_sync_value_id(&left.1);
        let right_id = jet_sync_value_id(&right.1);
        (left.0.as_str(), left_id).cmp(&(right.0.as_str(), right_id))
    });
    out
}

fn jet_sync_list_show_generic<T>(list: &JetSyncListGeneric<T>) -> String
where
    T: __jet_Encode,
{
    let parts = list
        .items
        .iter()
        .map(|(replica, item)| format!("{replica}:{}", jet_sync_value_show(item)))
        .collect::<Vec<_>>()
        .join("|");
    format!("SyncList({parts})")
}

fn jet_sync_list_metadata_generic<T>(list: &JetSyncListGeneric<T>) -> String {
    format!("GSet(items={})", list.items.len())
}

impl JetShow for JetSyncText {
    fn jet_show(&self) -> String { jet_sync_text_show(self) }
}

impl JetShow for JetSyncCounter {
    fn jet_show(&self) -> String { format!("{}={}", jet_sync_counter_metadata(self), jet_sync_counter_value(self)) }
}

impl JetShow for JetSyncMap {
    fn jet_show(&self) -> String { jet_sync_map_show(self) }
}

impl<K, V> JetShow for JetSyncMapGeneric<K, V>
where
    K: __jet_Encode,
    V: __jet_Encode,
{
    fn jet_show(&self) -> String { jet_sync_map_show_generic(self) }
}

impl JetShow for JetSyncList {
    fn jet_show(&self) -> String { jet_sync_list_show(self) }
}

impl<T> JetShow for JetSyncListGeneric<T>
where
    T: __jet_Encode,
{
    fn jet_show(&self) -> String { jet_sync_list_show_generic(self) }
}

fn jet_sync_decode_error(message: impl Into<String>) -> Vec<jet_std::FieldError> {
    jet_std::FieldError::one(message)
}

fn jet_sync_object<'a>(
    tree: &'a jet_std::DataTree,
    expected: &[&str],
    label: &str,
) -> Result<&'a Vec<(String, jet_std::DataTree)>, Vec<jet_std::FieldError>> {
    let jet_std::DataTree::Object(fields) = tree else {
        return Err(jet_sync_decode_error(format!("{label} must be an object")));
    };
    if fields.len() != expected.len()
        || fields.iter().any(|(key, _)| !expected.iter().any(|name| *name == key))
        || expected.iter().any(|name| !fields.iter().any(|(key, _)| key == name))
    {
        return Err(jet_sync_decode_error(format!(
            "{label} has missing, duplicate, or unknown fields"
        )));
    }
    Ok(fields)
}

fn jet_sync_object_field<'a>(
    fields: &'a [(String, jet_std::DataTree)],
    name: &str,
    label: &str,
) -> Result<&'a jet_std::DataTree, Vec<jet_std::FieldError>> {
    fields
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
        .ok_or_else(|| jet_sync_decode_error(format!("{label} is missing `{name}`")))
}

fn jet_sync_decode_string(
    tree: &jet_std::DataTree,
    label: &str,
) -> Result<String, Vec<jet_std::FieldError>> {
    match tree {
        jet_std::DataTree::Text(value) => Ok(value.clone()),
        other => Err(jet_sync_decode_error(format!(
            "{label} must be text, got {}",
            jet_std::datatree_kind(other)
        ))),
    }
}

fn jet_sync_decode_u64(
    tree: &jet_std::DataTree,
    label: &str,
) -> Result<u64, Vec<jet_std::FieldError>> {
    let value = match tree {
        jet_std::DataTree::Text(value) => value.clone(),
        jet_std::DataTree::Int(value) if *value >= 0 => value.to_string(),
        other => {
            return Err(jet_sync_decode_error(format!(
                "{label} must be a non-negative integer, got {}",
                jet_std::datatree_kind(other)
            )))
        }
    };
    if value.is_empty() || (value.len() > 1 && value.starts_with('0')) {
        return Err(jet_sync_decode_error(format!("{label} is not canonical")));
    }
    value
        .parse::<u64>()
        .map_err(|_| jet_sync_decode_error(format!("{label} is out of range")))
}

fn jet_sync_decode_array<'a>(
    tree: &'a jet_std::DataTree,
    label: &str,
) -> Result<&'a Vec<jet_std::DataTree>, Vec<jet_std::FieldError>> {
    match tree {
        jet_std::DataTree::Array(values) => Ok(values),
        other => Err(jet_sync_decode_error(format!(
            "{label} must be an array, got {}",
            jet_std::datatree_kind(other)
        ))),
    }
}

fn jet_sync_decode_field<T>(
    fields: &[(String, jet_std::DataTree)],
    name: &str,
    label: &str,
    errors: &mut Vec<jet_std::FieldError>,
    decode: impl FnOnce(&jet_std::DataTree) -> Result<T, Vec<jet_std::FieldError>>,
) -> Option<T> {
    match jet_sync_object_field(fields, name, label).and_then(decode) {
        Ok(value) => Some(value),
        Err(child) => {
            errors.extend(jet_std::FieldError::under_errors(name, child));
            None
        }
    }
}

fn jet_sync_frame_entry(
    errors: &mut Vec<jet_std::FieldError>,
    index: usize,
    entry_errors: Vec<jet_std::FieldError>,
) {
    errors.extend(jet_std::FieldError::under_errors(
        &format!("[{index}]"),
        entry_errors,
    ));
}

impl __jet_Encode for JetSyncText {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Object(vec![(
            "atoms".to_string(),
            jet_std::DataTree::Array(
                self.atoms
                    .iter()
                    .map(|atom| {
                        jet_std::DataTree::Object(vec![
                            (
                                "replica".to_string(),
                                jet_std::DataTree::Text(atom.replica.clone()),
                            ),
                            (
                                "counter".to_string(),
                                jet_std::DataTree::Text(atom.counter.to_string()),
                            ),
                            (
                                "after".to_string(),
                                jet_std::DataTree::Text(match &atom.after {
                                    Some((replica, counter)) => format!("{replica}:{counter}"),
                                    None => String::new(),
                                }),
                            ),
                            ("ch".to_string(), jet_std::DataTree::Text(atom.ch.to_string())),
                            (
                                "deleted".to_string(),
                                jet_std::DataTree::Bool(atom.deleted),
                            ),
                        ])
                    })
                    .collect(),
            ),
        )])
    }
}

impl __jet_Decode for JetSyncText {
    fn jet_decode(tree: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
        let fields = jet_sync_object(tree, &["atoms"], "SyncText")?;
        let values = jet_sync_decode_array(
            jet_sync_object_field(fields, "atoms", "SyncText")?,
            "SyncText.atoms",
        )?;
        if values.len() > MAX_SYNC_ATOMS {
            return Err(jet_sync_decode_error("SyncText atom limit exceeded"));
        }
        let mut atoms: Vec<JetSyncTextAtom> = Vec::with_capacity(values.len());
        let mut seen: std::collections::BTreeSet<(String, u64)> =
            std::collections::BTreeSet::new();
        let mut errors = Vec::new();
        for (index, value) in values.iter().enumerate() {
            let fields = match jet_sync_object(
                value,
                &["replica", "counter", "after", "ch", "deleted"],
                "SyncText atom",
            ) {
                Ok(fields) => fields,
                Err(entry_errors) => {
                    jet_sync_frame_entry(&mut errors, index, entry_errors);
                    continue;
                }
            };
            let mut entry_errors = Vec::new();
            let replica = jet_sync_decode_field(
                fields,
                "replica",
                "SyncText atom",
                &mut entry_errors,
                |tree| jet_sync_decode_string(tree, "SyncText.replica"),
            );
            let counter = jet_sync_decode_field(
                fields,
                "counter",
                "SyncText atom",
                &mut entry_errors,
                |tree| jet_sync_decode_u64(tree, "SyncText.counter"),
            );
            let after = jet_sync_decode_field(
                fields,
                "after",
                "SyncText atom",
                &mut entry_errors,
                |tree| jet_sync_decode_string(tree, "SyncText.after"),
            );
            let ch = jet_sync_decode_field(
                fields,
                "ch",
                "SyncText atom",
                &mut entry_errors,
                |tree| jet_sync_decode_string(tree, "SyncText.ch"),
            );
            let deleted = jet_sync_decode_field(
                fields,
                "deleted",
                "SyncText atom",
                &mut entry_errors,
                |tree| match tree {
                    jet_std::DataTree::Bool(value) => Ok(*value),
                    _ => Err(jet_sync_decode_error("SyncText.deleted expects a Bool")),
                },
            );
            if let (Some(replica), Some(counter), Some(after), Some(ch), Some(deleted)) =
                (replica, counter, after, ch, deleted)
            {
                let anchor = if after.is_empty() {
                    Ok(None)
                } else {
                    match after.rsplit_once(':') {
                        Some((replica, counter)) => match counter.parse::<u64>() {
                            Ok(counter) if jet_sync_token_is_valid(replica) && counter > 0 => {
                                Ok(Some((replica.to_string(), counter)))
                            }
                            _ => Err(()),
                        },
                        None => Err(()),
                    }
                };
                let mut chars = ch.chars();
                let single = match (chars.next(), chars.next()) {
                    (Some(ch), None) => Some(ch),
                    _ => None,
                };
                match (anchor, single) {
                    (Ok(anchor), Some(ch))
                        if jet_sync_token_is_valid(&replica) && counter > 0 =>
                    {
                        if !seen.insert((replica.clone(), counter)) {
                            entry_errors.extend(jet_sync_decode_error(
                                "SyncText contains duplicate atom identities",
                            ));
                        } else {
                            atoms.push(JetSyncTextAtom {
                                replica,
                                counter,
                                after: anchor,
                                ch,
                                deleted,
                            });
                        }
                    }
                    _ => {
                        entry_errors
                            .extend(jet_sync_decode_error("SyncText atom is invalid"));
                    }
                }
            }
            if !entry_errors.is_empty() {
                jet_sync_frame_entry(&mut errors, index, entry_errors);
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        // A character the reading order cannot reach would vanish from the
        // document with no error.  Presence of the anchor is not enough: two
        // atoms may anchor each other, or one may anchor itself, and both
        // shapes are unreachable.  So walk the real order and refuse anything
        // it does not visit.
        let decoded = JetSyncText {
            atoms: std::mem::take(&mut atoms),
        };
        if jet_sync_text_order(&decoded).len() != decoded.atoms.len() {
            return Err(jet_sync_decode_error(
                "SyncText carries a character the document cannot reach",
            ));
        }
        let mut atoms = decoded.atoms;
        atoms.sort_by(|left, right| {
            left.replica
                .cmp(&right.replica)
                .then_with(|| left.counter.cmp(&right.counter))
        });
        Ok(JetSyncText { atoms })
    }
}

impl __jet_Encode for JetSyncCounter {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Object(vec![
            (
                "counts".to_string(),
                jet_std::DataTree::Array(
                    self.counts
                        .iter()
                        .map(|(replica, positive, negative)| {
                            jet_std::DataTree::Object(vec![
                                ("replica".to_string(), jet_std::DataTree::Text(replica.clone())),
                                ("positive".to_string(), jet_std::DataTree::Text(positive.to_string())),
                                ("negative".to_string(), jet_std::DataTree::Text(negative.to_string())),
                            ])
                        })
                        .collect(),
                ),
            ),
        ])
    }
}

impl __jet_Decode for JetSyncCounter {
    fn jet_decode(tree: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
        let fields = jet_sync_object(tree, &["counts"], "SyncCounter")?;
        let values = jet_sync_decode_array(
            jet_sync_object_field(fields, "counts", "SyncCounter")?,
            "SyncCounter.counts",
        )?;
        if values.len() > MAX_SYNC_REPLICAS {
            return Err(jet_sync_decode_error("SyncCounter replica limit exceeded"));
        }
        let mut counts = Vec::with_capacity(values.len());
        let mut errors = Vec::new();
        for (index, value) in values.iter().enumerate() {
            let fields = jet_sync_object(
                value,
                &["replica", "positive", "negative"],
                "SyncCounter entry",
            );
            let fields = match fields {
                Ok(fields) => fields,
                Err(entry_errors) => {
                    jet_sync_frame_entry(&mut errors, index, entry_errors);
                    continue;
                }
            };
            let mut entry_errors = Vec::new();
            let replica = jet_sync_decode_field(
                fields,
                "replica",
                "SyncCounter entry",
                &mut entry_errors,
                |tree| jet_sync_decode_string(tree, "SyncCounter.replica"),
            );
            let positive = jet_sync_decode_field(
                fields,
                "positive",
                "SyncCounter entry",
                &mut entry_errors,
                |tree| jet_sync_decode_u64(tree, "SyncCounter.positive"),
            );
            let negative = jet_sync_decode_field(
                fields,
                "negative",
                "SyncCounter entry",
                &mut entry_errors,
                |tree| jet_sync_decode_u64(tree, "SyncCounter.negative"),
            );
            if let (Some(replica), Some(positive), Some(negative)) = (replica, positive, negative) {
                if !jet_sync_token_is_valid(&replica) {
                    entry_errors.extend(jet_sync_decode_error("SyncCounter replica is invalid"));
                } else if counts.iter().any(|(existing, _, _)| existing == &replica) {
                    entry_errors.extend(jet_sync_decode_error(
                        "SyncCounter contains duplicate replicas",
                    ));
                } else {
                    counts.push((replica, positive, negative));
                }
            }
            if !entry_errors.is_empty() {
                jet_sync_frame_entry(&mut errors, index, entry_errors);
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        counts.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(JetSyncCounter { counts })
    }
}

impl __jet_Encode for JetSyncMap {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Object(vec![
            (
                "entries".to_string(),
                jet_std::DataTree::Array(
                    self.entries
                        .iter()
                        .map(|(key, value, clock, writer)| {
                            jet_std::DataTree::Object(vec![
                                ("key".to_string(), jet_std::DataTree::Text(key.clone())),
                                ("value".to_string(), jet_std::DataTree::Text(value.clone())),
                                ("clock".to_string(), jet_std::DataTree::Text(clock.to_string())),
                                ("writer".to_string(), jet_std::DataTree::Text(writer.clone())),
                            ])
                        })
                        .collect(),
                ),
            ),
        ])
    }
}

impl __jet_Decode for JetSyncMap {
    fn jet_decode(tree: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
        let fields = jet_sync_object(tree, &["entries"], "SyncMap")?;
        let values = jet_sync_decode_array(
            jet_sync_object_field(fields, "entries", "SyncMap")?,
            "SyncMap.entries",
        )?;
        if values.len() > MAX_SYNC_ENTRIES {
            return Err(jet_sync_decode_error("SyncMap entry limit exceeded"));
        }
        let mut entries = Vec::with_capacity(values.len());
        let mut errors = Vec::new();
        for (index, value) in values.iter().enumerate() {
            let fields = match jet_sync_object(
                value,
                &["key", "value", "clock", "writer"],
                "SyncMap entry",
            ) {
                Ok(fields) => fields,
                Err(entry_errors) => {
                    jet_sync_frame_entry(&mut errors, index, entry_errors);
                    continue;
                }
            };
            let mut entry_errors = Vec::new();
            let key = jet_sync_decode_field(
                fields,
                "key",
                "SyncMap entry",
                &mut entry_errors,
                |tree| jet_sync_decode_string(tree, "SyncMap.key"),
            );
            let entry_value = jet_sync_decode_field(
                fields,
                "value",
                "SyncMap entry",
                &mut entry_errors,
                |tree| jet_sync_decode_string(tree, "SyncMap.value"),
            );
            let clock = jet_sync_decode_field(
                fields,
                "clock",
                "SyncMap entry",
                &mut entry_errors,
                |tree| jet_sync_decode_u64(tree, "SyncMap.clock"),
            );
            let writer = jet_sync_decode_field(
                fields,
                "writer",
                "SyncMap entry",
                &mut entry_errors,
                |tree| jet_sync_decode_string(tree, "SyncMap.writer"),
            );
            if let (Some(key), Some(entry_value), Some(clock), Some(writer)) =
                (key, entry_value, clock, writer)
            {
                if !jet_sync_token_is_valid(&key)
                    || !jet_sync_token_is_valid(&writer)
                    || entry_value.len() > MAX_SYNC_TEXT
                    || clock == 0
                {
                    entry_errors.extend(jet_sync_decode_error("SyncMap entry is invalid"));
                } else if entries.iter().any(|(existing, _, _, _)| existing == &key) {
                    entry_errors.extend(jet_sync_decode_error(
                        "SyncMap contains duplicate keys",
                    ));
                } else {
                    entries.push((key, entry_value, clock, writer));
                }
            }
            if !entry_errors.is_empty() {
                jet_sync_frame_entry(&mut errors, index, entry_errors);
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(JetSyncMap { entries })
    }
}

impl __jet_Encode for JetSyncList {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Object(vec![
            (
                "items".to_string(),
                jet_std::DataTree::Array(
                    self.items
                        .iter()
                        .map(|(replica, item)| {
                            jet_std::DataTree::Object(vec![
                                ("replica".to_string(), jet_std::DataTree::Text(replica.clone())),
                                ("item".to_string(), jet_std::DataTree::Text(item.clone())),
                            ])
                        })
                        .collect(),
                ),
            ),
        ])
    }
}

impl __jet_Decode for JetSyncList {
    fn jet_decode(tree: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
        let fields = jet_sync_object(tree, &["items"], "SyncList")?;
        let values = jet_sync_decode_array(
            jet_sync_object_field(fields, "items", "SyncList")?,
            "SyncList.items",
        )?;
        if values.len() > MAX_SYNC_ENTRIES {
            return Err(jet_sync_decode_error("SyncList item limit exceeded"));
        }
        let mut items = Vec::with_capacity(values.len());
        let mut errors = Vec::new();
        for (index, value) in values.iter().enumerate() {
            let fields = match jet_sync_object(value, &["replica", "item"], "SyncList item") {
                Ok(fields) => fields,
                Err(entry_errors) => {
                    jet_sync_frame_entry(&mut errors, index, entry_errors);
                    continue;
                }
            };
            let mut entry_errors = Vec::new();
            let replica = jet_sync_decode_field(
                fields,
                "replica",
                "SyncList item",
                &mut entry_errors,
                |tree| jet_sync_decode_string(tree, "SyncList.replica"),
            );
            let item = jet_sync_decode_field(
                fields,
                "item",
                "SyncList item",
                &mut entry_errors,
                |tree| jet_sync_decode_string(tree, "SyncList.item"),
            );
            if let (Some(replica), Some(item)) = (replica, item) {
                if !jet_sync_token_is_valid(&replica) || item.len() > MAX_SYNC_TEXT {
                    entry_errors.extend(jet_sync_decode_error("SyncList item is invalid"));
                } else if items.iter().any(|(existing_replica, existing_item)| {
                    existing_replica == &replica && existing_item == &item
                }) {
                    entry_errors.extend(jet_sync_decode_error("SyncList contains duplicate items"));
                } else {
                    items.push((replica, item));
                }
            }
            if !entry_errors.is_empty() {
                jet_sync_frame_entry(&mut errors, index, entry_errors);
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        items.sort_by(|left, right| (&left.0, &left.1).cmp(&(&right.0, &right.1)));
        Ok(JetSyncList { items })
    }
}

impl<K, V> __jet_Encode for JetSyncMapGeneric<K, V>
where
    K: __jet_Encode,
    V: __jet_Encode,
{
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Object(vec![
            (
                "entries".to_string(),
                jet_std::DataTree::Array(
                    self.entries
                        .iter()
                        .map(|(key, value, clock, writer)| {
                            jet_std::DataTree::Object(vec![
                                ("key".to_string(), key.jet_encode()),
                                ("value".to_string(), value.jet_encode()),
                                ("clock".to_string(), jet_std::DataTree::Text(clock.to_string())),
                                ("writer".to_string(), jet_std::DataTree::Text(writer.clone())),
                            ])
                        })
                        .collect(),
                ),
            ),
        ])
    }
}

impl<K, V> __jet_Decode for JetSyncMapGeneric<K, V>
where
    K: __jet_Decode + __jet_Encode,
    V: __jet_Decode + __jet_Encode,
{
    fn jet_decode(tree: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
        let fields = jet_sync_object(tree, &["entries"], "SyncMap")?;
        let values = jet_sync_decode_array(
            jet_sync_object_field(fields, "entries", "SyncMap")?,
            "SyncMap.entries",
        )?;
        if values.len() > MAX_SYNC_ENTRIES {
            return Err(jet_sync_decode_error("SyncMap entry limit exceeded"));
        }
        let mut entries = Vec::with_capacity(values.len());
        let mut errors = Vec::new();
        for (index, value) in values.iter().enumerate() {
            let fields = match jet_sync_object(
                value,
                &["key", "value", "clock", "writer"],
                "SyncMap entry",
            ) {
                Ok(fields) => fields,
                Err(entry_errors) => {
                    jet_sync_frame_entry(&mut errors, index, entry_errors);
                    continue;
                }
            };
            let mut entry_errors = Vec::new();
            let key = jet_sync_decode_field(
                fields,
                "key",
                "SyncMap entry",
                &mut entry_errors,
                K::jet_decode,
            );
            let entry_value = jet_sync_decode_field(
                fields,
                "value",
                "SyncMap entry",
                &mut entry_errors,
                V::jet_decode,
            );
            let clock = jet_sync_decode_field(
                fields,
                "clock",
                "SyncMap entry",
                &mut entry_errors,
                |tree| jet_sync_decode_u64(tree, "SyncMap.clock"),
            );
            let writer = jet_sync_decode_field(
                fields,
                "writer",
                "SyncMap entry",
                &mut entry_errors,
                |tree| jet_sync_decode_string(tree, "SyncMap.writer"),
            );
            if let (Some(key), Some(entry_value), Some(clock), Some(writer)) =
                (key, entry_value, clock, writer)
            {
                if !jet_sync_token_is_valid(&writer) || clock == 0 {
                    entry_errors.extend(jet_sync_decode_error("SyncMap entry is invalid"));
                } else {
                    let key_id = jet_sync_value_id(&key);
                    if entries
                        .iter()
                        .any(|(existing_key, _, _, _)| jet_sync_value_id(existing_key) == key_id)
                    {
                        entry_errors.extend(jet_sync_decode_error(
                            "SyncMap contains duplicate keys",
                        ));
                    } else {
                        entries.push((key, entry_value, clock, writer));
                    }
                }
            }
            if !entry_errors.is_empty() {
                jet_sync_frame_entry(&mut errors, index, entry_errors);
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        entries.sort_by(|left, right| {
            jet_sync_value_id(&left.0).cmp(&jet_sync_value_id(&right.0))
        });
        Ok(JetSyncMapGeneric { entries })
    }
}

impl<T> __jet_Encode for JetSyncListGeneric<T>
where
    T: __jet_Encode,
{
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Object(vec![
            (
                "items".to_string(),
                jet_std::DataTree::Array(
                    self.items
                        .iter()
                        .map(|(replica, item)| {
                            jet_std::DataTree::Object(vec![
                                ("replica".to_string(), jet_std::DataTree::Text(replica.clone())),
                                ("item".to_string(), item.jet_encode()),
                            ])
                        })
                        .collect(),
                ),
            ),
        ])
    }
}

impl<T> __jet_Decode for JetSyncListGeneric<T>
where
    T: __jet_Decode + __jet_Encode,
{
    fn jet_decode(tree: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
        let fields = jet_sync_object(tree, &["items"], "SyncList")?;
        let values = jet_sync_decode_array(
            jet_sync_object_field(fields, "items", "SyncList")?,
            "SyncList.items",
        )?;
        if values.len() > MAX_SYNC_ENTRIES {
            return Err(jet_sync_decode_error("SyncList item limit exceeded"));
        }
        let mut items = Vec::with_capacity(values.len());
        let mut errors = Vec::new();
        for (index, value) in values.iter().enumerate() {
            let fields = match jet_sync_object(value, &["replica", "item"], "SyncList item") {
                Ok(fields) => fields,
                Err(entry_errors) => {
                    jet_sync_frame_entry(&mut errors, index, entry_errors);
                    continue;
                }
            };
            let mut entry_errors = Vec::new();
            let replica = jet_sync_decode_field(
                fields,
                "replica",
                "SyncList item",
                &mut entry_errors,
                |tree| jet_sync_decode_string(tree, "SyncList.replica"),
            );
            let item = jet_sync_decode_field(
                fields,
                "item",
                "SyncList item",
                &mut entry_errors,
                T::jet_decode,
            );
            if let (Some(replica), Some(item)) = (replica, item) {
                if !jet_sync_token_is_valid(&replica) {
                    entry_errors.extend(jet_sync_decode_error("SyncList item is invalid"));
                } else {
                    let item_id = jet_sync_value_id(&item);
                    if items.iter().any(|(existing_replica, existing_item)| {
                        existing_replica == &replica && jet_sync_value_id(existing_item) == item_id
                    }) {
                        entry_errors.extend(jet_sync_decode_error(
                            "SyncList contains duplicate items",
                        ));
                    } else {
                        items.push((replica, item));
                    }
                }
            }
            if !entry_errors.is_empty() {
                jet_sync_frame_entry(&mut errors, index, entry_errors);
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        items.sort_by(|left, right| {
            let left_id = jet_sync_value_id(&left.1);
            let right_id = jet_sync_value_id(&right.1);
            (left.0.as_str(), left_id).cmp(&(right.0.as_str(), right_id))
        });
        Ok(JetSyncListGeneric { items })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetSyncDocument {
    representation: String,
}

impl JetSyncDocument {
    fn parse(value: String) -> Option<Self> {
        let representation = value.trim().to_string();
        if representation.is_empty()
            || representation.len() > MAX_SYNC_DOCUMENT
            || representation.chars().any(char::is_control)
            || ![
                "SyncText(",
                "SyncMap(",
                "SyncList(",
                "PNCounter(",
                "LWWMap(",
                "GSet(",
            ]
            .iter()
            .any(|prefix| representation.starts_with(prefix))
        {
            return None;
        }
        Some(Self { representation })
    }
}

#[derive(Clone, Debug)]
struct JetSyncReceipt {
    session_id: String,
    generation: u64,
    document: JetSyncDocument,
}

impl JetSyncReceipt {
    fn show(&self) -> String {
        format!(
            "SyncOver(session={}, generation={}, doc={})",
            self.session_id, self.generation, self.document.representation
        )
    }
}

#[derive(Clone, Debug)]
struct JetSyncSessionState {
    generation: u64,
    document: JetSyncDocument,
}

static JET_SYNC_SESSIONS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::BTreeMap<String, JetSyncSessionState>>,
> = std::sync::OnceLock::new();

fn jet_sync_sessions(
) -> &'static std::sync::Mutex<std::collections::BTreeMap<String, JetSyncSessionState>> {
    JET_SYNC_SESSIONS.get_or_init(|| std::sync::Mutex::new(std::collections::BTreeMap::new()))
}

/// Publish a canonical CRDT document onto the named sync session. The registry
/// is the shared runtime adapter for AOT and CTFE. Duplicate delivery is
/// idempotent: it returns the existing receipt instead of advancing the
/// generation. The String return is only the fixed Core compatibility boundary;
/// the session state and receipt are typed here.
fn jet_sync_publish(session_id: String, doc_show: String) -> String {
    if session_id.trim().is_empty()
        || session_id.len() > MAX_SYNC_SESSION
        || session_id.chars().any(char::is_control)
    {
        return "SyncError(invalid session)".to_string();
    }
    let Some(document) = JetSyncDocument::parse(doc_show) else {
        return "SyncError(document is not a canonical CRDT value)".to_string();
    };
    let mut sessions = match jet_sync_sessions().lock() {
        Ok(sessions) => sessions,
        Err(_) => return "SyncError(session registry is unavailable)".to_string(),
    };
    if !sessions.contains_key(&session_id) && sessions.len() >= MAX_SYNC_ENTRIES {
        return "SyncError(session limit exceeded)".to_string();
    }
    let state = sessions
        .entry(session_id.clone())
        .or_insert_with(|| JetSyncSessionState {
            generation: 0,
            document: document.clone(),
        });
    if state.document != document {
        state.generation = state.generation.saturating_add(1);
        state.document = document;
    } else if state.generation == 0 {
        state.generation = 1;
    }
    JetSyncReceipt {
        session_id,
        generation: state.generation,
        document: state.document.clone(),
    }
    .show()
}

pub(crate) fn jet_app_sync_over(session_id: String, doc_show: String) -> String {
    jet_sync_publish(session_id, doc_show)
}

/// D-SYNC1: the ratified surface puts the document first and names the
/// session with `over:`. Keep the legacy `sync_over(session, doc)` adapter for
/// compatibility, but give the canonical spelling its own typed entry point.
pub(crate) fn jet_app_sync(doc_show: String, session_id: String) -> String {
    jet_app_sync_over(session_id, doc_show)
}
