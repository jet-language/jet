// D-SYNC1=A / D-DBPOLICY1=A (#1159/#1160): CRDT values + typed row policies.

#[derive(Clone, Debug)]
pub struct JetSyncText {
    pub replicas: Vec<(String, String)>, // replica_id, text
}

#[derive(Clone, Debug)]
pub struct JetSyncCounter {
    pub counts: Vec<(String, i64)>,
}

#[derive(Clone, Debug)]
pub struct JetSyncMap {
    pub entries: Vec<(String, String)>,
}

#[derive(Clone, Debug)]
pub struct JetRowPolicy {
    pub table: String,
    pub expression: String,
}

#[derive(Clone, Debug)]
pub struct JetSyncList {
    pub items: Vec<(String, String)>, // replica_id, serialized item
}

fn jet_sync_text_new(replica: String, text: String) -> JetSyncText {
    JetSyncText {
        replicas: vec![(replica, text)],
    }
}

fn jet_sync_text_set(mut doc: JetSyncText, replica: String, text: String) -> JetSyncText {
    if let Some((_, existing)) = doc.replicas.iter_mut().find(|(r, _)| r == &replica) {
        *existing = text;
    } else {
        doc.replicas.push((replica, text));
    }
    doc
}

fn jet_sync_text_merge(a: &JetSyncText, b: &JetSyncText) -> JetSyncText {
    let mut out = a.clone();
    for (replica, text) in &b.replicas {
        if let Some((_, existing)) = out.replicas.iter_mut().find(|(r, _)| r == replica) {
            // Deterministic LWW by lexicographic max of payload.
            if text > existing {
                *existing = text.clone();
            }
        } else {
            out.replicas.push((replica.clone(), text.clone()));
        }
    }
    out.replicas.sort_by(|l, r| l.0.cmp(&r.0));
    out
}

fn jet_sync_text_show(doc: &JetSyncText) -> String {
    let parts = doc
        .replicas
        .iter()
        .map(|(r, t)| format!("{r}:{t}"))
        .collect::<Vec<_>>()
        .join("|");
    format!("SyncText({parts})")
}

fn jet_sync_counter_new(replica: String, value: i64) -> JetSyncCounter {
    JetSyncCounter {
        counts: vec![(replica, value)],
    }
}

fn jet_sync_counter_inc(mut counter: JetSyncCounter, replica: String, delta: i64) -> JetSyncCounter {
    if let Some((_, v)) = counter.counts.iter_mut().find(|(r, _)| r == &replica) {
        *v += delta;
    } else {
        counter.counts.push((replica, delta));
    }
    counter
}

fn jet_sync_counter_merge(a: &JetSyncCounter, b: &JetSyncCounter) -> JetSyncCounter {
    let mut out = a.clone();
    for (replica, value) in &b.counts {
        if let Some((_, existing)) = out.counts.iter_mut().find(|(r, _)| r == replica) {
            *existing = (*existing).max(*value);
        } else {
            out.counts.push((replica.clone(), *value));
        }
    }
    out
}

fn jet_sync_counter_value(counter: &JetSyncCounter) -> i64 {
    counter.counts.iter().map(|(_, v)| *v).sum()
}

fn jet_sync_map_new() -> JetSyncMap {
    JetSyncMap {
        entries: Vec::new(),
    }
}

fn jet_sync_map_set(mut map: JetSyncMap, key: String, value: String) -> JetSyncMap {
    if let Some((_, v)) = map.entries.iter_mut().find(|(k, _)| k == &key) {
        *v = value;
    } else {
        map.entries.push((key, value));
    }
    map
}

fn jet_sync_map_get(map: &JetSyncMap, key: &String) -> Option<String> {
    map.entries
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
}

fn jet_sync_map_merge(a: &JetSyncMap, b: &JetSyncMap) -> JetSyncMap {
    let mut out = a.clone();
    for (k, v) in &b.entries {
        if let Some((_, existing)) = out.entries.iter_mut().find(|(ek, _)| ek == k) {
            if v > existing {
                *existing = v.clone();
            }
        } else {
            out.entries.push((k.clone(), v.clone()));
        }
    }
    out.entries.sort_by(|l, r| l.0.cmp(&r.0));
    out
}

fn jet_sync_map_show(map: &JetSyncMap) -> String {
    let parts = map
        .entries
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("SyncMap({parts})")
}

fn jet_db_policy_new(table: String, expression: String) -> Result<JetRowPolicy, String> {
    if table.trim().is_empty() || expression.trim().is_empty() {
        return Err("row policy needs a table and expression".to_string());
    }
    Ok(JetRowPolicy { table, expression })
}

fn jet_db_policy_allows(policy: &JetRowPolicy, user: &String, row_owner: &String) -> bool {
    // Beginner default expression shape: `owner == user`.
    if policy.expression.contains("owner == user") {
        return user == row_owner;
    }
    // Explicit allow-all expert expression.
    if policy.expression.trim() == "true" {
        return true;
    }
    false
}

fn jet_db_policy_show(policy: &JetRowPolicy) -> String {
    format!("RowPolicy(table={}, expr={})", policy.table, policy.expression)
}

fn jet_sync_list_new() -> JetSyncList {
    JetSyncList { items: Vec::new() }
}

fn jet_sync_list_push(mut list: JetSyncList, replica: String, item: String) -> JetSyncList {
    list.items.push((replica, item));
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
    out.items.sort_by(|l, r| (&l.0, &l.1).cmp(&(&r.0, &r.1)));
    out
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

fn jet_app_sync_over(session_id: String, doc_show: String) -> String {
    format!("SyncOver(session={session_id}, doc={doc_show})")
}
