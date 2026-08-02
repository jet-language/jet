// D-SYNC1=A / D-DBPOLICY1=A (#1159/#1160): CRDT values + typed row policies.

const MAX_SYNC_TEXT: usize = 1024 * 1024;
const MAX_SYNC_REPLICAS: usize = 4096;
const MAX_SYNC_ENTRIES: usize = 100_000;
const MAX_SYNC_SESSION: usize = 256;
const MAX_SYNC_DOCUMENT: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct JetSyncText {
    pub replicas: Vec<(String, String, u64)>, // replica_id, text, logical clock
}

#[derive(Clone, Debug)]
pub struct JetSyncCounter {
    pub counts: Vec<(String, u64, u64)>, // replica_id, positive, negative
}

#[derive(Clone, Debug)]
pub struct JetSyncMap {
    pub entries: Vec<(String, String, u64, String)>, // key, value, clock, writer
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

#[derive(Clone, Debug)]
pub struct JetSyncList {
    pub items: Vec<(String, String)>, // replica_id, serialized item
}

fn jet_sync_text_new(replica: String, text: String) -> JetSyncText {
    if !jet_sync_token_is_valid(&replica) || text.len() > MAX_SYNC_TEXT {
        return JetSyncText { replicas: Vec::new() };
    }
    JetSyncText {
        replicas: vec![(replica, text, 1)],
    }
}

fn jet_sync_text_set(mut doc: JetSyncText, replica: String, text: String) -> JetSyncText {
    if !jet_sync_token_is_valid(&replica) || text.len() > MAX_SYNC_TEXT {
        return doc;
    }
    let next_clock = doc
        .replicas
        .iter()
        .map(|(_, _, clock)| *clock)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    if let Some((_, existing, clock)) = doc.replicas.iter_mut().find(|(r, _, _)| r == &replica) {
        *existing = text;
        *clock = next_clock;
    } else {
        if doc.replicas.len() >= MAX_SYNC_REPLICAS {
            return doc;
        }
        doc.replicas.push((replica, text, next_clock));
    }
    doc
}

fn jet_sync_text_merge(a: &JetSyncText, b: &JetSyncText) -> JetSyncText {
    let mut merged = std::collections::BTreeMap::<String, (String, u64)>::new();
    for (replica, text, clock) in a.replicas.iter().chain(&b.replicas) {
        if !jet_sync_token_is_valid(replica) || text.len() > MAX_SYNC_TEXT {
            continue;
        }
        let replace = merged.get(replica).map_or(true, |(existing, existing_clock)| {
            *clock > *existing_clock
                || (*clock == *existing_clock && text.as_str() > existing.as_str())
        });
        if replace {
            merged.insert(replica.clone(), (text.clone(), *clock));
        }
    }
    JetSyncText {
        replicas: merged
            .into_iter()
            .take(MAX_SYNC_REPLICAS)
            .map(|(replica, (text, clock))| (replica, text, clock))
            .collect(),
    }
}

fn jet_sync_text_show(doc: &JetSyncText) -> String {
    let parts = doc
        .replicas
        .iter()
        .map(|(r, t, _)| format!("{r}:{t}"))
        .collect::<Vec<_>>()
        .join("|");
    format!("SyncText({parts})")
}

fn jet_sync_counter_new(replica: String, value: i64) -> JetSyncCounter {
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

fn jet_sync_counter_inc(mut counter: JetSyncCounter, replica: String, delta: i64) -> JetSyncCounter {
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

fn jet_sync_counter_merge(a: &JetSyncCounter, b: &JetSyncCounter) -> JetSyncCounter {
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

fn jet_sync_counter_value(counter: &JetSyncCounter) -> i64 {
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

fn jet_sync_map_new() -> JetSyncMap {
    JetSyncMap {
        entries: Vec::new(),
    }
}

fn jet_sync_map_set(mut map: JetSyncMap, key: String, value: String) -> JetSyncMap {
    if !jet_sync_token_is_valid(&key) || value.len() > MAX_SYNC_TEXT {
        return map;
    }
    let next_clock = map
        .entries
        .iter()
        .map(|(_, _, clock, _)| *clock)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    if let Some((_, v, clock, writer)) = map.entries.iter_mut().find(|(k, _, _, _)| k == &key) {
        *v = value;
        *clock = next_clock;
        *writer = "local".to_string();
    } else {
        if map.entries.len() >= MAX_SYNC_ENTRIES {
            return map;
        }
        map.entries.push((key, value, next_clock, "local".to_string()));
    }
    map
}

fn jet_sync_map_get(map: &JetSyncMap, key: &String) -> Option<String> {
    map.entries
        .iter()
        .find(|(k, _, _, _)| k == key)
        .map(|(_, v, _, _)| v.clone())
}

fn jet_sync_map_merge(a: &JetSyncMap, b: &JetSyncMap) -> JetSyncMap {
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

fn jet_sync_map_show(map: &JetSyncMap) -> String {
    let parts = map
        .entries
        .iter()
        .map(|(k, v, _, _)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("SyncMap({parts})")
}

fn jet_db_policy_new(table: String, expression: String) -> Result<JetRowPolicy, String> {
    if table.trim().is_empty()
        || expression.trim().is_empty()
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

fn jet_db_policy_allows(policy: &JetRowPolicy, user: &String, row_owner: &String) -> bool {
    if !jet_sync_token_is_valid(user) || !jet_sync_token_is_valid(row_owner) {
        return false;
    }
    match &policy.compiled {
        JetRowPolicyExpr::OwnerEqualsUser => user == row_owner,
        JetRowPolicyExpr::AllowAll => true,
    }
}

fn jet_db_policy_show(policy: &JetRowPolicy) -> String {
    format!("RowPolicy(table={}, expr={})", policy.table, policy.expression)
}

fn jet_sync_list_new() -> JetSyncList {
    JetSyncList { items: Vec::new() }
}

fn jet_sync_list_push(mut list: JetSyncList, replica: String, item: String) -> JetSyncList {
    if !jet_sync_token_is_valid(&replica) || item.len() > MAX_SYNC_TEXT {
        return list;
    }
    if !list.items.iter().any(|(r, i)| r == &replica && i == &item) {
        if list.items.len() < MAX_SYNC_ENTRIES {
            list.items.push((replica, item));
        }
    }
    list
}

fn jet_sync_list_merge(a: &JetSyncList, b: &JetSyncList) -> JetSyncList {
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

fn jet_sync_token_is_valid(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= MAX_SYNC_TEXT
        && !value.chars().any(char::is_control)
}

fn jet_sync_list_show(list: &JetSyncList) -> String {
    let parts = list
        .items
        .iter()
        .map(|(r, i)| format!("{r}:{i}"))
        .collect::<Vec<_>>()
        .join("|");
    format!("SyncList({parts})")
}

#[derive(Clone, Debug)]
struct JetSyncSessionState {
    generation: u64,
    document: String,
}

static JET_SYNC_SESSIONS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::BTreeMap<String, JetSyncSessionState>>,
> = std::sync::OnceLock::new();

fn jet_sync_sessions(
) -> &'static std::sync::Mutex<std::collections::BTreeMap<String, JetSyncSessionState>> {
    JET_SYNC_SESSIONS.get_or_init(|| std::sync::Mutex::new(std::collections::BTreeMap::new()))
}

/// Publish a document onto the named live-sync session. The registry is the
/// shared runtime adapter for AOT and CTFE: it gives `app.sync` a real,
/// serialized session state instead of returning a display-only string. The
/// document remains opaque here because typed CRDT values own their merge law;
/// this boundary only records the latest merged representation and a monotonic
/// receipt generation.
fn jet_sync_publish(session_id: String, doc_show: String) -> String {
    if session_id.trim().is_empty()
        || session_id.len() > MAX_SYNC_SESSION
        || session_id.chars().any(char::is_control)
    {
        return "SyncError(invalid session)".to_string();
    }
    if doc_show.len() > MAX_SYNC_DOCUMENT || doc_show.chars().any(char::is_control) {
        return "SyncError(document exceeds sync limit)".to_string();
    }
    let mut sessions = jet_sync_sessions().lock().unwrap();
    if !sessions.contains_key(&session_id) && sessions.len() >= MAX_SYNC_ENTRIES {
        return "SyncError(session limit exceeded)".to_string();
    }
    let state = sessions
        .entry(session_id.clone())
        .or_insert_with(|| JetSyncSessionState {
            generation: 0,
            document: String::new(),
        });
    state.generation = state.generation.saturating_add(1);
    state.document = doc_show.clone();
    format!(
        "SyncOver(session={session_id}, generation={}, doc={})",
        state.generation, state.document
    )
}

fn jet_app_sync_over(session_id: String, doc_show: String) -> String {
    jet_sync_publish(session_id, doc_show)
}

/// D-SYNC1: the ratified surface puts the document first and names the
/// session with `over:`. Keep the legacy `sync_over(session, doc)` adapter for
/// compatibility, but give the canonical spelling its own typed entry point.
fn jet_app_sync(doc_show: String, session_id: String) -> String {
    jet_app_sync_over(session_id, doc_show)
}
