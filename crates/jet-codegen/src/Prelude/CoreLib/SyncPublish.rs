#[derive(Clone, Debug, PartialEq, Eq)]
struct JetSyncDocument {
    representation: String,
}

fn jet_sync_escape_display(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(ch, '\\' | ',' | '=' | '|' | ':' | '/') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

fn jet_sync_unescape_display(value: &str) -> Option<String> {
    let mut unescaped = String::with_capacity(value.len());
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            if !matches!(ch, '\\' | ',' | '=' | '|' | ':' | '/') {
                return None;
            }
            unescaped.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if matches!(ch, ',' | '=' | '|' | ':' | '/') {
            return None;
        } else {
            unescaped.push(ch);
        }
    }
    (!escaped).then_some(unescaped)
}

fn jet_sync_split_display(value: &str, separator: char) -> Option<Vec<String>> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            current.push('\\');
            current.push(ch);
            escaped = false;
        } else if ch == '\\' {
            current.push(ch);
            escaped = true;
        } else if ch == separator {
            parts.push(std::mem::take(&mut current));
        } else {
            current.push(ch);
        }
    }
    if escaped {
        return None;
    }
    parts.push(current);
    Some(parts)
}

fn jet_sync_split_display_once(value: &str, separator: char) -> Option<(String, String)> {
    let mut escaped = false;
    for (index, ch) in value.char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == separator {
            return Some((
                value[..index].to_string(),
                value[index + ch.len_utf8()..].to_string(),
            ));
        }
    }
    None
}

fn jet_sync_document_body<'a>(representation: &'a str, prefix: &str) -> Option<&'a str> {
    representation
        .strip_prefix(prefix)
        .and_then(|value| value.strip_suffix(')'))
}

fn jet_sync_document_key_valid(value: &str, forbidden: &[char]) -> bool {
    jet_sync_token_is_valid(value) && !value.chars().any(|ch| forbidden.contains(&ch))
}

fn jet_sync_document_map_entries(
    representation: &str,
) -> Option<std::collections::BTreeMap<String, String>> {
    let body = jet_sync_document_body(representation, "SyncMap(")?;
    let mut entries = std::collections::BTreeMap::new();
    if body.is_empty() {
        return Some(entries);
    }
    for entry in jet_sync_split_display(body, ',')? {
        let (raw_key, raw_value) = jet_sync_split_display_once(&entry, '=')?;
        let key = jet_sync_unescape_display(&raw_key)?;
        let value = jet_sync_unescape_display(&raw_value)?;
        if !jet_sync_document_key_valid(&key, &[]) || value.len() > MAX_SYNC_TEXT {
            return None;
        }
        if entries.insert(key, value).is_some() {
            return None;
        }
    }
    Some(entries)
}

fn jet_sync_document_list_items(
    representation: &str,
) -> Option<std::collections::BTreeSet<(String, String)>> {
    let body = jet_sync_document_body(representation, "SyncList(")?;
    let mut items = std::collections::BTreeSet::new();
    if body.is_empty() {
        return Some(items);
    }
    for item in jet_sync_split_display(body, '|')? {
        let (raw_replica, raw_value) = jet_sync_split_display_once(&item, ':')?;
        let replica = jet_sync_unescape_display(&raw_replica)?;
        let value = jet_sync_unescape_display(&raw_value)?;
        if !jet_sync_document_key_valid(&replica, &[]) || value.len() > MAX_SYNC_TEXT {
            return None;
        }
        if !items.insert((replica, value)) {
            return None;
        }
    }
    (items.len() <= MAX_SYNC_ENTRIES).then_some(items)
}

fn jet_sync_document_counter_entries(
    representation: &str,
) -> Option<std::collections::BTreeMap<String, (u64, u64)>> {
    let body = jet_sync_document_body(representation, "PNCounter(")?;
    let mut counts = std::collections::BTreeMap::new();
    if body.is_empty() {
        return Some(counts);
    }
    for entry in jet_sync_split_display(body, ',')? {
        let (raw_replica, raw_value) = jet_sync_split_display_once(&entry, '=')?;
        let replica = jet_sync_unescape_display(&raw_replica)?;
        let (raw_positive, raw_negative) = jet_sync_split_display_once(&raw_value, '/')?;
        let positive = jet_sync_unescape_display(&raw_positive)?
            .strip_prefix('+')?
            .parse::<u64>()
            .ok()?;
        let negative = jet_sync_unescape_display(&raw_negative)?
            .strip_prefix('-')?
            .parse::<u64>()
            .ok()?;
        if !jet_sync_document_key_valid(&replica, &[])
            || counts.insert(replica, (positive, negative)).is_some()
        {
            return None;
        }
    }
    if counts.len() > MAX_SYNC_REPLICAS {
        return None;
    }
    let total = counts
        .iter()
        .map(|(replica, (positive, negative))| (replica.clone(), *positive, *negative))
        .collect::<Vec<_>>();
    jet_sync_counter_total(&total)?;
    Some(counts)
}

impl JetSyncDocument {
    fn parse(value: String) -> Option<Self> {
        let representation = value.trim().to_string();
        if representation.is_empty()
            || representation != value
            || representation.len() > MAX_SYNC_DOCUMENT
            || representation.chars().any(char::is_control)
            || !["SyncText(", "SyncMap(", "SyncList(", "PNCounter("]
                .iter()
                .any(|prefix| representation.starts_with(prefix))
        {
            return None;
        }
        if !representation.ends_with(')') {
            return None;
        }
        let canonical = if representation.starts_with("SyncMap(") {
            let entries = jet_sync_document_map_entries(&representation)?;
            format!(
                "SyncMap({})",
                entries
                    .into_iter()
                    .map(|(key, value)| {
                        format!(
                            "{}={}",
                            jet_sync_escape_display(&key),
                            jet_sync_escape_display(&value)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            )
        } else if representation.starts_with("SyncList(") {
            let items = jet_sync_document_list_items(&representation)?;
            format!(
                "SyncList({})",
                items
                    .into_iter()
                    .map(|(replica, value)| {
                        format!(
                            "{}:{}",
                            jet_sync_escape_display(&replica),
                            jet_sync_escape_display(&value)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("|")
            )
        } else if representation.starts_with("PNCounter(") {
            let counts = jet_sync_document_counter_entries(&representation)?;
            format!(
                "PNCounter({})",
                counts
                    .into_iter()
                    .map(|(replica, (positive, negative))| {
                        format!(
                            "{}=+{positive}/-{negative}",
                            jet_sync_escape_display(&replica)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            )
        } else {
            representation.clone()
        };
        if canonical != representation {
            return None;
        }
        Some(Self { representation })
    }

    /// The typed CRDT carriers merge by identity above this String boundary.
    /// `app.sync` still receives their display, so the boundary may merge only
    /// facts that survive display serialization: map keys with equal values,
    /// list members, and counter components. A conflicting map value or an
    /// opaque text document is denied instead of being silently replaced by a
    /// lexical winner. Malformed displays are denied as well.
    fn merge(&self, other: &Self) -> Option<Self> {
        if self.representation == other.representation {
            return Some(self.clone());
        }
        let merged = if self.representation.starts_with("SyncMap(")
            && other.representation.starts_with("SyncMap(")
        {
            let mut entries = jet_sync_document_map_entries(&self.representation)?;
            for (key, value) in jet_sync_document_map_entries(&other.representation)? {
                match entries.get(&key) {
                    Some(existing) if existing != &value => return None,
                    Some(_) => {}
                    None => {
                        entries.insert(key, value);
                    }
                }
            }
            if entries.len() > MAX_SYNC_ENTRIES {
                return None;
            }
            format!(
                "SyncMap({})",
                entries
                    .into_iter()
                    .map(|(key, value)| {
                        format!(
                            "{}={}",
                            jet_sync_escape_display(&key),
                            jet_sync_escape_display(&value)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            )
        } else if self.representation.starts_with("SyncList(")
            && other.representation.starts_with("SyncList(")
        {
            let mut items = jet_sync_document_list_items(&self.representation)?;
            items.extend(jet_sync_document_list_items(&other.representation)?);
            if items.len() > MAX_SYNC_ENTRIES {
                return None;
            }
            format!(
                "SyncList({})",
                items
                    .into_iter()
                    .map(|(replica, value)| {
                        format!(
                            "{}:{}",
                            jet_sync_escape_display(&replica),
                            jet_sync_escape_display(&value)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("|")
            )
        } else if self.representation.starts_with("PNCounter(")
            && other.representation.starts_with("PNCounter(")
        {
            let mut counts = jet_sync_document_counter_entries(&self.representation)?;
            for (replica, (positive, negative)) in
                jet_sync_document_counter_entries(&other.representation)?
            {
                let slot = counts.entry(replica).or_default();
                slot.0 = slot.0.max(positive);
                slot.1 = slot.1.max(negative);
            }
            if counts.len() > MAX_SYNC_REPLICAS {
                return None;
            }
            format!(
                "PNCounter({})",
                counts
                    .into_iter()
                    .map(|(replica, (positive, negative))| {
                        format!(
                            "{}=+{positive}/-{negative}",
                            jet_sync_escape_display(&replica)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            )
        } else {
            return None;
        };
        Self::parse(merged)
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
    let mut changed = false;
    if state.document != document {
        let Some(merged) = state.document.merge(&document) else {
            return "SyncError(document merge denied)".to_string();
        };
        if state.document != merged {
            let Some(generation) = state.generation.checked_add(1) else {
                return "SyncError(session generation exhausted)".to_string();
            };
            state.generation = generation;
            state.document = merged;
            changed = true;
        }
    } else if state.generation == 0 {
        state.generation = 1;
        changed = true;
    }
    let receipt = JetSyncReceipt {
        session_id,
        generation: state.generation,
        document: state.document.clone(),
    };
    let shown = receipt.show();
    let transport = if changed {
        Some(format!(
            "sync:{}:{}:{}",
            receipt.session_id, receipt.generation, receipt.document.representation
        ))
    } else {
        None
    };
    drop(sessions);
    if let Some(transport) = transport {
        jet_live_publish_transport(format!("sync:{}", receipt.session_id), transport);
    }
    shown
}

/// D-SYNC1: the ratified surface puts the document first and names the
/// session with `over:`. The fixed Core boundary carries the canonical display
/// while the session registry applies the typed merge law before publishing.
pub(crate) fn jet_app_sync(doc_show: String, session_id: String) -> String {
    jet_sync_publish(session_id, doc_show)
}
