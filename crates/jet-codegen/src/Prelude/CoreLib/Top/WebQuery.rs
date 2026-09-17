// D-DX-QUERY1=A (#2473): one Query lifecycle over app.live and core.reactive.
//
// Live database reads keep their normalized DB.Read footprint in LiveQuery.
// Non-database sources use the explicit ext: identity installed by app.subscribe.
// This module owns query state, mutation rollback, offline replay, and dev facts;
// execution tiers only marshal the resulting handles and values.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc as JetWebQueryArc, LazyLock, Mutex as JetWebQueryMutex};
use std::time::{SystemTime, UNIX_EPOCH};
use jet_foundation::Devtools::{
    JetDevtoolsEvent,
    JetDevtoolsMutationLifecycle,
};

const JET_WEB_QUERY_MAX_VALUE: usize = 4 * 1024 * 1024;
const JET_WEB_QUERY_MAX_QUEUE: usize = 1024;
const JET_WEB_QUERY_MAX_KEY: usize = 512;
const JET_WEB_QUERY_MAX_CONTEXT: usize = 512;
const JET_WEB_QUERY_MAX_INVALIDATIONS: usize = 64;
const JET_WEB_QUERY_MAX_INVALIDATION_TARGET: usize = 512;
const JET_WEB_QUERY_QUEUE_FORMAT: &str = "jet-web-query-queue-v1";

type JetWebQueryFetch = JetWebQueryArc<dyn Fn() -> Result<String, String> + Send + Sync + 'static>;

type JetWebQueryOfflineQueue = VecDeque<JetWebQueryQueuedMutation>;

#[derive(Clone, Debug, Eq, PartialEq)]
struct JetWebQueryQueuedMutation {
    sequence: u64,
    key: String,
    payload: String,
    invalidations: Vec<String>,
}

#[derive(Default)]
struct JetWebQueryQueueState {
    loaded: bool,
    load_error: Option<String>,
    next_sequence: u64,
    entries: JetWebQueryOfflineQueue,
}

static JET_WEB_QUERY_OFFLINE_QUEUE: LazyLock<JetWebQueryMutex<JetWebQueryQueueState>> =
    LazyLock::new(|| JetWebQueryMutex::new(JetWebQueryQueueState::default()));

fn jet_web_query_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}
fn jet_web_query_queue_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("JET_WEB_QUERY_QUEUE_PATH") {
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))?;
    Some(base.join("jet").join("web-query-queue.v1"))
}

fn jet_web_query_hex(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(value.len().saturating_mul(2));
    for byte in value.as_bytes() {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn jet_web_query_unhex(value: &str) -> Option<String> {
    fn nibble(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }
    if value.len() % 2 != 0 {
        return None;
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks_exact(2) {
        bytes.push((nibble(pair[0])? << 4) | nibble(pair[1])?);
    }
    String::from_utf8(bytes).ok()
}

fn jet_web_query_durability_error(detail: impl std::fmt::Display) -> String {
    format!("offline mutation durability unavailable: {detail}")
}

fn jet_web_query_queue_load(queue: &mut JetWebQueryQueueState) -> Result<(), String> {
    if queue.loaded {
        return queue
            .load_error
            .clone()
            .map_or(Ok(()), Err);
    }
    queue.loaded = true;
    let Some(path) = jet_web_query_queue_path() else {
        let error = jet_web_query_durability_error("no state directory is available");
        queue.load_error = Some(error.clone());
        return Err(error);
    };
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            let error = jet_web_query_durability_error(error);
            queue.load_error = Some(error.clone());
            return Err(error);
        }
    };
    let parsed = (|| -> Result<(), String> {
        let mut lines = contents.lines();
        if lines.next() != Some(JET_WEB_QUERY_QUEUE_FORMAT) {
            return Err(jet_web_query_durability_error(
                "queue file has an unknown format",
            ));
        }
        for line in lines.filter(|line| !line.is_empty()) {
            let fields: Vec<&str> = line.split('\t').collect();
            match fields.as_slice() {
                ["next", sequence] => {
                    queue.next_sequence = sequence.parse::<u64>().map_err(|_| {
                        jet_web_query_durability_error("queue sequence is not an integer")
                    })?;
                }
                ["entry", sequence, key, payload, invalidations] => {
                    if queue.entries.len() >= JET_WEB_QUERY_MAX_QUEUE {
                        return Err(jet_web_query_durability_error(
                            "queue file exceeds the entry limit",
                        ));
                    }
                    let sequence = sequence.parse::<u64>().map_err(|_| {
                        jet_web_query_durability_error(
                            "queued mutation sequence is not an integer",
                        )
                    })?;
                    let key = jet_web_query_unhex(key).ok_or_else(|| {
                        jet_web_query_durability_error(
                            "queued mutation key is not valid UTF-8",
                        )
                    })?;
                    let payload = jet_web_query_unhex(payload).ok_or_else(|| {
                        jet_web_query_durability_error(
                            "queued mutation payload is not valid UTF-8",
                        )
                    })?;
                    let invalidations = if invalidations.is_empty() {
                        Vec::new()
                    } else {
                        invalidations
                            .split(',')
                            .map(|target| {
                                jet_web_query_unhex(target).ok_or_else(|| {
                                    jet_web_query_durability_error(
                                        "queued invalidation target is not valid UTF-8",
                                    )
                                })
                            })
                            .collect::<Result<Vec<_>, _>>()?
                    };
                    queue.next_sequence = queue.next_sequence.max(sequence);
                    queue.entries.push_back(JetWebQueryQueuedMutation {
                        sequence,
                        key,
                        payload,
                        invalidations,
                    });
                }
                _ => {
                    return Err(jet_web_query_durability_error(
                        "queue file contains an invalid row",
                    ));
                }
            }
        }
        Ok(())
    })();
    if let Err(error) = parsed {
        queue.load_error = Some(error.clone());
        return Err(error);
    }
    Ok(())
}

fn jet_web_query_queue_persist(queue: &JetWebQueryQueueState) -> Result<(), String> {
    let Some(path) = jet_web_query_queue_path() else {
        return Err(jet_web_query_durability_error("no state directory is available"));
    };
    let mut contents = format!("{JET_WEB_QUERY_QUEUE_FORMAT}\nnext\t{}\n", queue.next_sequence);
    for entry in &queue.entries {
        let invalidations = entry
            .invalidations
            .iter()
            .map(|target| jet_web_query_hex(target))
            .collect::<Vec<_>>()
            .join(",");
        contents.push_str(&format!(
            "entry\t{}\t{}\t{}\t{}\n",
            entry.sequence,
            jet_web_query_hex(&entry.key),
            jet_web_query_hex(&entry.payload),
            invalidations,
        ));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(jet_web_query_durability_error)?;
    }
    let temp = path.with_extension(format!("v1.tmp.{}", std::process::id()));
    std::fs::write(&temp, contents).map_err(jet_web_query_durability_error)?;
    if let Err(error) = std::fs::rename(&temp, &path) {
        let _ = std::fs::remove_file(&temp);
        return Err(jet_web_query_durability_error(error));
    }
    Ok(())
}

fn jet_web_query_json(value: &str) -> String {
    format!("{value:?}")
}

fn jet_web_query_bound_error(error: String) -> String {
    if error.len() <= JET_WEB_QUERY_MAX_VALUE {
        error
    } else {
        "query error exceeds the payload limit".to_string()
    }
}

fn jet_web_query_valid_key(key: &str) -> bool {
    !key.trim().is_empty()
        && key.len() <= JET_WEB_QUERY_MAX_KEY
        && key.chars().all(|character| !character.is_control())
}
fn jet_web_query_targets(targets: Vec<String>) -> Result<Vec<String>, String> {
    if targets.len() > JET_WEB_QUERY_MAX_INVALIDATIONS {
        return Err(format!(
            "query invalidation list exceeds {} entries",
            JET_WEB_QUERY_MAX_INVALIDATIONS
        ));
    }
    let mut normalized: Vec<String> = Vec::with_capacity(targets.len());
    for target in targets {
        let target = target.trim();
        if target.is_empty()
            || target.len() > JET_WEB_QUERY_MAX_INVALIDATION_TARGET
            || target.chars().any(char::is_control)
        {
            return Err("query invalidation target is invalid".to_string());
        }
        if target
            .strip_prefix("key:")
            .is_some_and(|key| key.trim().is_empty())
        {
            return Err("query invalidation target is invalid".to_string());
        }
        if !normalized.iter().any(|existing| existing.as_str() == target) {
            normalized.push(target.to_string());
        }
    }
    Ok(normalized)
}

fn jet_web_query_valid_source(source: &str) -> bool {
    !source.trim().is_empty()
        && source.len() <= JET_WEB_QUERY_MAX_KEY
        && source.chars().all(|character| !character.is_control())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetWebMutationStatus {
    Idle,
    Pending,
    Success,
    Error,
    Settled,
}

impl JetWebMutationStatus {
    pub fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Pending => "pending",
            Self::Success => "success",
            Self::Error => "error",
            Self::Settled => "settled",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetWebMutationState {
    pub status: JetWebMutationStatus,
    pub generation: u64,
    pub optimistic: bool,
    pub value: String,
    pub result: String,
    pub error: String,
    pub context: String,
    pub rollback: bool,
    pub paused: bool,
    pub queued: usize,
    pub replayed: u64,
}

impl JetWebMutationState {
    fn idle() -> Self {
        Self {
            status: JetWebMutationStatus::Idle,
            generation: 0,
            optimistic: false,
            value: String::new(),
            result: String::new(),
            error: String::new(),
            context: String::new(),
            rollback: false,
            paused: false,
            queued: 0,
            replayed: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetWebQueryStatus {
    Pending,
    Fresh,
    Stale,
    Fetching,
    Error,
    Offline,
}

impl JetWebQueryStatus {
    pub fn name(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Fresh => "fresh",
            Self::Stale => "stale",
            Self::Fetching => "fetching",
            Self::Error => "error",
            Self::Offline => "offline",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetWebQueryNetworkMode {
    Online,
    Always,
    OfflineFirst,
}

impl JetWebQueryNetworkMode {
    pub fn name(self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Always => "always",
            Self::OfflineFirst => "offlineFirst",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetWebQueryState {
    pub status: JetWebQueryStatus,
    pub value: String,
    pub error: String,
    pub generation: u64,
    pub queued: usize,
    pub offline: bool,
}

impl JetWebQueryState {
    fn fresh(value: String) -> Self {
        Self {
            status: JetWebQueryStatus::Fresh,
            value,
            error: String::new(),
            generation: 1,
            queued: 0,
            offline: false,
        }
    }
}

#[derive(Clone)]
pub struct JetWebQuery {
    key: String,
    footprint: String,
    state: jet_std::JetSignal<JetWebQueryState>,
    mutation: jet_std::JetSignal<JetWebMutationState>,
    live: Option<JetLiveQuery>,
    fetch: Option<JetWebQueryFetch>,
    mode: JetWebQueryArc<JetWebQueryMutex<JetWebQueryNetworkMode>>,
    online: JetWebQueryArc<JetWebQueryMutex<bool>>,
    mutation_lock: JetWebQueryArc<JetWebQueryMutex<()>>,
}

#[derive(Clone)]
struct JetWebQueryPanelFact {
    key: String,
    footprint: String,
    state: jet_std::JetSignal<JetWebQueryState>,
    mutation: jet_std::JetSignal<JetWebMutationState>,
    live: Option<JetLiveQuery>,
    mode: JetWebQueryArc<JetWebQueryMutex<JetWebQueryNetworkMode>>,
}

static JET_WEB_QUERY_PANEL_FACTS: LazyLock<JetWebQueryMutex<Vec<JetWebQueryPanelFact>>> =
    LazyLock::new(|| JetWebQueryMutex::new(Vec::new()));

fn jet_web_query_register_panel_fact(query: &JetWebQuery) {
    let Ok(mut facts) = JET_WEB_QUERY_PANEL_FACTS.lock() else {
        return;
    };
    if let Some(fact) = facts.iter_mut().find(|fact| fact.key == query.key) {
        fact.footprint = query.footprint.clone();
        fact.state = query.state.clone();
        fact.mutation = query.mutation.clone();
        fact.live = query.live.clone();
        fact.mode = query.mode.clone();
        return;
    }
    facts.push(JetWebQueryPanelFact {
        key: query.key.clone(),
        footprint: query.footprint.clone(),
        state: query.state.clone(),
        mutation: query.mutation.clone(),
        live: query.live.clone(),
        mode: query.mode.clone(),
    });
}

fn jet_web_query_html_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Development-only query facts. Values and errors stay in the signal; the
/// panel exposes only identity, lifecycle, and byte/count metadata.
pub(crate) fn jet_web_query_dev_panel() -> String {
    let Ok(facts) = JET_WEB_QUERY_PANEL_FACTS.lock() else {
        return String::new();
    };
    let mut rows = Vec::new();
    for fact in facts.iter() {
        let state = fact.state.get();
        let mutation = fact.mutation.get();
        let mode = fact
            .mode
            .lock()
            .map(|mode| mode.name())
            .unwrap_or("unknown");
        let (generation, observers, freshness_ms, cause) = fact
            .live
            .as_ref()
            .and_then(|live| {
                let Ok(registry) = jet_live_registry().lock() else {
                    return None;
                };
                let stored = registry.queries.get(&live.id)?;
                Some((
                    stored.lifecycle.generation,
                    if stored.sink.is_some() { 1 } else { 0 },
                    jet_live_age_ms(stored.lifecycle.fresh_at_ms),
                    stored.lifecycle.invalidation_cause.clone(),
                ))
            })
            .unwrap_or((state.generation, 0, 0, "none".to_string()));
        let facts_text = format!(
            "key={},footprint={},state={},generation={},freshness_ms={},observers={},cause={},mode={},mutation={},queued={},value_bytes={},error_bytes={}",
            fact.key,
            fact.footprint,
            jet_web_query_dev_state(state.status),
            generation,
            freshness_ms,
            observers,
            cause,
            mode,
            mutation.status.name(),
            state.queued,
            state.value.len(),
            state.error.len(),
        );
        rows.push(format!(
            "<li data-jet-query-key=\"{}\"><code>{}</code><pre>{}</pre></li>",
            jet_web_query_html_attr(&fact.key),
            jet_web_query_html_attr(&fact.key),
            jet_web_query_html_attr(&facts_text),
        ));
    }
    if rows.is_empty() {
        return String::new();
    }
    format!(
        "<aside id=\"jet-dev-queries\" data-jet-devtools=\"queries\"><h2>Queries</h2><ul>{}</ul></aside>",
        rows.join("")
    )
}

pub(crate) fn jet_web_query_facts_json() -> String {
    let Ok(facts) = JET_WEB_QUERY_PANEL_FACTS.lock() else {
        return "[]".to_string();
    };
    let items = facts
        .iter()
        .map(|fact| {
            let kind = if fact.footprint.starts_with("ext:") {
                "subscribe"
            } else {
                "live"
            };
            format!(
                "{{\"key\":{},\"footprint\":{},\"source\":\"runtime\",\"kind\":{}}}",
                jet_web_query_json(&fact.key),
                jet_web_query_json(&fact.footprint),
                jet_web_query_json(kind),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("[{items}]")
}

fn jet_web_query_dev_state(status: JetWebQueryStatus) -> &'static str {
    match status {
        JetWebQueryStatus::Pending => "pending",
        JetWebQueryStatus::Fresh => "fresh",
        JetWebQueryStatus::Stale => "stale",
        JetWebQueryStatus::Fetching => "fetching",
        JetWebQueryStatus::Error => "error",
        JetWebQueryStatus::Offline => "offline",
    }
}

fn jet_web_query_publish_state(
    key: &str,
    footprint: &str,
    state: &JetWebQueryState,
    observers: i64,
    cause: &str,
    mutation: JetWebMutationStatus,
    freshness_ms: u64,
) {
    if !jet_web_runtime_devtools_enabled() {
        return;
    }
    let state_name = jet_web_query_dev_state(state.status);
    let fields = format!(
        "{{\"name\":{},\"state\":{},\"footprint\":{},\"freshness_ms\":{},\"observers\":{},\"invalidation_cause\":{},\"mutation_state\":{}}}",
        jet_web_query_json(key),
        jet_web_query_json(state_name),
        jet_web_query_json(footprint),
        freshness_ms,
        observers.max(0),
        jet_web_query_json(cause),
        jet_web_query_json(mutation.name()),
    );
    if let Ok(event) = JetDevtoolsEvent::from_parts(
        jet_web_query_now_ms(),
        "core.web.query",
        "Query",
        key.to_string(),
        fields,
    ) {
        jet_devtools_publish_event(event);
    }
}

fn jet_web_query_publish_mutation(
    key: &str,
    lifecycle: JetDevtoolsMutationLifecycle,
    rollback: bool,
) {
    if !jet_web_runtime_devtools_enabled() {
        return;
    }
    let lifecycle_name = match lifecycle {
        JetDevtoolsMutationLifecycle::Started => "started",
        JetDevtoolsMutationLifecycle::Committed => "committed",
        JetDevtoolsMutationLifecycle::Failed => "failed",
        JetDevtoolsMutationLifecycle::RolledBack => "rolled_back",
    };
    let fields = format!(
        "{{\"name\":{},\"lifecycle\":{},\"rollback\":{}}}",
        jet_web_query_json(key),
        jet_web_query_json(lifecycle_name),
        rollback,
    );
    if let Ok(event) = JetDevtoolsEvent::from_parts(
        jet_web_query_now_ms(),
        "core.web.query",
        "Mutation",
        key.to_string(),
        fields,
    ) {
        jet_devtools_publish_event(event);
    }
}

fn jet_web_query_queue_count(key: &str) -> usize {
    let Ok(mut queue) = JET_WEB_QUERY_OFFLINE_QUEUE.lock() else {
        return 0;
    };
    if jet_web_query_queue_load(&mut queue).is_err() {
        return 0;
    }
    queue.entries.iter().filter(|entry| entry.key == key).count()
}

fn jet_web_query_enqueue(
    key: &str,
    payload: String,
    invalidations: Vec<String>,
) -> Result<usize, String> {
    let mut queue = JET_WEB_QUERY_OFFLINE_QUEUE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    jet_web_query_queue_load(&mut queue)?;
    let key_count = queue.entries.iter().filter(|entry| entry.key == key).count();
    if queue.entries.len() >= JET_WEB_QUERY_MAX_QUEUE || key_count >= JET_WEB_QUERY_MAX_QUEUE {
        return Err("offline query queue is full".to_string());
    }
    let previous_sequence = queue.next_sequence;
    queue.next_sequence = queue.next_sequence.saturating_add(1).max(1);
    let sequence = queue.next_sequence;
    queue.entries.push_back(JetWebQueryQueuedMutation {
        sequence,
        key: key.to_string(),
        payload,
        invalidations,
    });
    if let Err(error) = jet_web_query_queue_persist(&queue) {
        let _ = queue.entries.pop_back();
        queue.next_sequence = previous_sequence;
        return Err(error);
    }
    Ok(key_count.saturating_add(1))
}

fn jet_web_query_queued(key: &str) -> Result<Vec<JetWebQueryQueuedMutation>, String> {
    let mut queue = JET_WEB_QUERY_OFFLINE_QUEUE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    jet_web_query_queue_load(&mut queue)?;
    Ok(queue
        .entries
        .iter()
        .filter(|entry| entry.key == key)
        .cloned()
        .collect())
}

fn jet_web_query_remove_queued(key: &str, sequence: u64) -> Result<(), String> {
    let mut queue = JET_WEB_QUERY_OFFLINE_QUEUE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    jet_web_query_queue_load(&mut queue)?;
    let Some(index) = queue
        .entries
        .iter()
        .position(|entry| entry.key == key && entry.sequence == sequence)
    else {
        return Ok(());
    };
    let removed = queue.entries.remove(index);
    if let Err(error) = jet_web_query_queue_persist(&queue) {
        if let Some(removed) = removed {
            queue.entries.insert(index, removed);
        }
        return Err(error);
    }
    Ok(())
}

impl JetWebQuery {
    pub fn new(key: String, initial: String) -> Self {
        let invalid_key = !jet_web_query_valid_key(&key);
        let oversized = initial.len() > JET_WEB_QUERY_MAX_VALUE;
        let value = if oversized { String::new() } else { initial };
        let mut initial_state = JetWebQueryState::fresh(value);
        if invalid_key {
            initial_state.status = JetWebQueryStatus::Error;
            initial_state.error = format!(
                "query key is empty, exceeds {} bytes, or contains control characters",
                JET_WEB_QUERY_MAX_KEY
            );
        } else if oversized {
            initial_state.status = JetWebQueryStatus::Error;
            initial_state.error = format!(
                "query payload exceeds {} bytes",
                JET_WEB_QUERY_MAX_VALUE
            );
        }
        Self {
            key,
            footprint: String::new(),
            state: jet_std::JetSignal::new(initial_state),
            mutation: jet_std::JetSignal::new(JetWebMutationState::idle()),
            live: None,
            fetch: None,
            mode: JetWebQueryArc::new(JetWebQueryMutex::new(JetWebQueryNetworkMode::Online)),
            online: JetWebQueryArc::new(JetWebQueryMutex::new(true)),
            mutation_lock: JetWebQueryArc::new(JetWebQueryMutex::new(())),
        }
    }
    fn bind_live_state(&self, live: &JetLiveQuery) -> JetLiveQuery {
        let signal = self.state.clone();
        let key = self.key.clone();
        let footprint = self.footprint.clone();
        jet_app_live_bind_sink(
            live,
            JetWebQueryArc::new(move |value| {
                let before = signal.get();
                let state = JetWebQueryState {
                    status: if before.offline {
                        JetWebQueryStatus::Offline
                    } else {
                        JetWebQueryStatus::Fresh
                    },
                    value,
                    error: String::new(),
                    generation: before.generation.saturating_add(1),
                    ..before
                };
                signal.set(state.clone());
                jet_web_query_publish_state(
                    &key,
                    &footprint,
                    &state,
                    0,
                    "live-push",
                    JetWebMutationStatus::Idle,
                    0,
                );
            }),
        )
    }



    pub fn live<F>(key: String, footprint: String, initial: String, fetch: F) -> Self
    where
        F: Fn() -> Result<String, String> + Send + Sync + 'static,
    {
        let query = Self::new(key, initial);
        if !query.state.get().error.is_empty() {
            jet_web_query_register_panel_fact(&query);
            return query;
        }
        let initial_fetch = query.state.get().value.is_empty();
        let signal = query.state.clone();
        let key_for_fetch = query.key.clone();
        if initial_fetch {
            let mut pending = query.state.get();
            pending.status = JetWebQueryStatus::Pending;
            query.state.set(pending);
        }
        let footprint_for_fetch = footprint.clone();
        let fetch: JetWebQueryFetch = JetWebQueryArc::new(fetch);
        let fetch_for_live = fetch.clone();
        let live = jet_app_live_keyed_query(
            query.key.clone(),
            footprint.clone(),
            signal.get().value.clone(),
            move || {
                let before = signal.get();
                let fetching = JetWebQueryState {
                    status: JetWebQueryStatus::Fetching,
                    error: String::new(),
                    ..before
                };
                signal.set(fetching.clone());
                jet_web_query_publish_state(
                    &key_for_fetch,
                    &footprint_for_fetch,
                    &fetching,
                    0,
                    "fetch-start",
                    JetWebMutationStatus::Idle,
                    0,
                );
                match fetch_for_live() {
                    Ok(value) if value.len() <= JET_WEB_QUERY_MAX_VALUE => {
                        let before = signal.get();
                        let fresh = JetWebQueryState {
                            status: JetWebQueryStatus::Fresh,
                            value: value.clone(),
                            error: String::new(),
                            generation: before.generation.saturating_add(1),
                            ..before
                        };
                        signal.set(fresh.clone());
                        jet_web_query_publish_state(
                            &key_for_fetch,
                            &footprint_for_fetch,
                            &fresh,
                            0,
                            "fetch-success",
                            JetWebMutationStatus::Idle,
                            0,
                        );
                        Ok(value)
                    }
                    Ok(_) => {
                        let error = format!(
                            "query payload exceeds {} bytes",
                            JET_WEB_QUERY_MAX_VALUE
                        );
                        let before = signal.get();
                        let failed = JetWebQueryState {
                            status: JetWebQueryStatus::Error,
                            error: error.clone(),
                            ..before
                        };
                        signal.set(failed.clone());
                        jet_web_query_publish_state(
                            &key_for_fetch,
                            &footprint_for_fetch,
                            &failed,
                            0,
                            "fetch-error",
                            JetWebMutationStatus::Idle,
                            0,
                        );
                        Err(error)
                    }
                    Err(error) => {
                        let error = jet_web_query_bound_error(error);
                        let before = signal.get();
                        let failed = JetWebQueryState {
                            status: JetWebQueryStatus::Error,
                            error: error.clone(),
                            ..before
                        };
                        signal.set(failed.clone());
                        jet_web_query_publish_state(
                            &key_for_fetch,
                            &footprint_for_fetch,
                            &failed,
                            0,
                            "fetch-error",
                            JetWebMutationStatus::Idle,
                            0,
                        );
                        Err(error)
                    }
                }
            },
        );
        let signal_for_sink = query.state.clone();
        let key_for_sink = query.key.clone();
        let footprint_for_sink = footprint.clone();
        let live = jet_app_live_bind_sink(
            &live,
            JetWebQueryArc::new(move |value| {
                let before = signal_for_sink.get();
                let state = JetWebQueryState {
                    status: if before.offline {
                        JetWebQueryStatus::Offline
                    } else {
                        JetWebQueryStatus::Fresh
                    },
                    value,
                    error: String::new(),
                    generation: before.generation.saturating_add(1),
                    ..before
                };
                signal_for_sink.set(state.clone());
                jet_web_query_publish_state(
                    &key_for_sink,
                    &footprint_for_sink,
                    &state,
                    0,
                    "live-commit",
                    JetWebMutationStatus::Idle,
                    0,
                );
            }),
        );
        let mut result = query;
        result.footprint = footprint;
        result.live = Some(live);
        result.fetch = Some(fetch);
        result.sync_live_state();
        if initial_fetch {
            let _ = result.refresh();
        }
        jet_web_query_register_panel_fact(&result);
        result
    }

    pub fn subscribe(source: String) -> Self {
        let query = Self::new(source.clone(), String::new());
        if !jet_web_query_valid_source(&source) {
            let mut state = query.state.get();
            state.status = JetWebQueryStatus::Error;
            state.error = "subscription source is empty, too large, or contains control characters".to_string();
            query.state.set(state);
            jet_web_query_register_panel_fact(&query);
            return query;
        }
        let live = jet_app_subscribe(source.clone());
        let mut query = query;
        query.footprint = format!("ext:{source}");
        let live = query.bind_live_state(&live);
        query.live = Some(live);
        query.sync_live_state();
        jet_web_query_register_panel_fact(&query);
        query
    }

    pub fn key(&self) -> String {
        self.key.clone()
    }

    pub fn footprint(&self) -> String {
        self.footprint.clone()
    }

    pub fn state(&self) -> JetWebQueryState {
        self.sync_live_state();
        self.state.get()
    }

    pub fn state_signal(&self) -> jet_std::JetSignal<JetWebQueryState> {
        self.state.clone()
    }

    pub fn mutation_state(&self) -> JetWebMutationState {
        self.mutation.get()
    }

    pub fn mutation_signal(&self) -> jet_std::JetSignal<JetWebMutationState> {
        self.mutation.clone()
    }

    pub fn mode(&self) -> JetWebQueryNetworkMode {
        self.mode
            .lock()
            .map(|mode| *mode)
            .unwrap_or(JetWebQueryNetworkMode::Online)
    }

    pub fn set_mode(&self, mode: JetWebQueryNetworkMode) {
        if let Ok(mut current) = self.mode.lock() {
            *current = mode;
        }
        let state = self.state();
        self.publish_state(&state, "policy-changed");
    }

    pub fn is_online(&self) -> bool {
        self.online.lock().map(|online| *online).unwrap_or(false)
    }

    pub fn set_online(&self, online: bool) {
        if let Ok(mut current) = self.online.lock() {
            *current = online;
        }
        let mut state = self.state.get();
        let queued = jet_web_query_queue_count(&self.key);
        state.queued = queued;
        state.offline = !online || queued > 0;
        state.status = if queued > 0 || !online {
            JetWebQueryStatus::Offline
        } else if !state.error.is_empty() {
            JetWebQueryStatus::Error
        } else {
            match state.status {
                JetWebQueryStatus::Stale | JetWebQueryStatus::Fetching => state.status,
                JetWebQueryStatus::Pending if state.value.is_empty() => JetWebQueryStatus::Pending,
                _ if state.value.is_empty() => JetWebQueryStatus::Pending,
                _ => JetWebQueryStatus::Fresh,
            }
        };
        self.set_state(state, "connectivity-changed");
        self.sync_live_state();
    }
    fn invalidate_targets(&self, targets: &[String]) {
        for target in targets {
            if let Some(key) = target.strip_prefix("key:") {
                let _ = jet_app_invalidate_key(key.to_string());
            } else {
                let _ = jet_app_invalidate(target.clone());
            }
        }
    }


    pub fn mutate<F>(
        &self,
        payload: String,
        optimistic: Option<String>,
        action: F,
    ) -> JetWebMutationState
    where
        F: Fn(String) -> Result<String, String>,
    {
        self.mutate_with_invalidations(payload, optimistic, Vec::new(), action)
    }

    pub fn mutate_with_invalidations<F>(
        &self,
        payload: String,
        optimistic: Option<String>,
        invalidations: Vec<String>,
        action: F,
    ) -> JetWebMutationState
    where
        F: Fn(String) -> Result<String, String>,
    {
        let _mutation_lock = self
            .mutation_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = self.state();
        let generation = self.mutation.get().generation.saturating_add(1);
        let optimistic_present = optimistic.is_some();
        if payload.len() > JET_WEB_QUERY_MAX_VALUE {
            return self.finish_mutation_error(
                generation,
                false,
                String::new(),
                "mutation payload exceeds the payload limit".to_string(),
                false,
                0,
            );
        }
        if let Some(value) = optimistic.as_ref() {
            if value.len() > JET_WEB_QUERY_MAX_VALUE {
                return self.finish_mutation_error(
                    generation,
                    false,
                    String::new(),
                    "optimistic query value exceeds the payload limit".to_string(),
                    false,
                    0,
                );
            }
        }
        let targets = if invalidations.is_empty() {
            // A query's declared footprint is the dependency set for its
            // mutation. Invalidate that set by default so every query that
            // read the affected data refreshes, while unrelated footprints
            // remain untouched. Plain `new` handles have no dependency
            // metadata, so they retain the explicit-key fallback.
            if self.footprint.trim().is_empty() {
                if self.key.is_empty() {
                    Vec::new()
                } else {
                    vec![format!("key:{}", self.key)]
                }
            } else {
                vec![self.footprint.clone()]
            }
        } else {
            invalidations
        };
        let targets = match jet_web_query_targets(targets) {
            Ok(targets) => targets,
            Err(error) => {
                return self.finish_mutation_error(
                    generation,
                    optimistic_present,
                    self.mutation_context(previous.generation),
                    error,
                    false,
                    0,
                )
            }
        };
        let mode = self.mode();
        if !self.is_online() && mode == JetWebQueryNetworkMode::OfflineFirst {
            let queue_count = match jet_web_query_enqueue(&self.key, payload, targets.clone()) {
                Ok(count) => count,
                Err(error) => {
                    return self.finish_mutation_error(
                        generation,
                        false,
                        String::new(),
                        error,
                        false,
                        0,
                    )
                }
            };
            if let Some(value) = optimistic.clone() {
                let state = JetWebQueryState {
                    status: JetWebQueryStatus::Offline,
                    value,
                    error: String::new(),
                    generation: previous.generation.saturating_add(1),
                    queued: queue_count,
                    offline: true,
                };
                self.set_state(state, "optimistic-paused");
            }
            let pending = JetWebMutationState {
                status: JetWebMutationStatus::Pending,
                generation,
                optimistic: optimistic_present,
                value: self.state().value,
                result: String::new(),
                error: String::new(),
                context: self.mutation_context(previous.generation),
                rollback: false,
                paused: true,
                queued: queue_count,
                replayed: self.mutation.get().replayed,
            };
            self.set_mutation(pending.clone());
            return pending;
        }
        if !self.is_online() && mode == JetWebQueryNetworkMode::Online {
            return self.finish_mutation_error(
                generation,
                optimistic_present,
                self.mutation_context(previous.generation),
                "query is offline".to_string(),
                false,
                0,
            );
        }
        let context = self.mutation_context(previous.generation);
        let optimistic_value = optimistic.clone().unwrap_or_else(|| previous.value.clone());
        let pending = JetWebMutationState {
            status: JetWebMutationStatus::Pending,
            generation,
            optimistic: optimistic_present,
            value: optimistic_value,
            result: String::new(),
            error: String::new(),
            context: context.clone(),
            rollback: false,
            paused: false,
            queued: 0,
            replayed: self.mutation.get().replayed,
        };
        self.set_mutation(pending);
        if let Some(value) = optimistic.clone() {
            let state = JetWebQueryState {
                status: if previous.offline {
                    JetWebQueryStatus::Offline
                } else {
                    JetWebQueryStatus::Stale
                },
                value,
                error: String::new(),
                generation: previous.generation.saturating_add(1),
                ..previous.clone()
            };
            self.set_state(state, "optimistic-applied");
        }
        let outcome = action(payload);
        let (status, result, error, rollback) = match outcome {
            Ok(result) if result.len() <= JET_WEB_QUERY_MAX_VALUE => {
                (JetWebMutationStatus::Success, result, String::new(), false)
            }
            Ok(_) => (
                JetWebMutationStatus::Error,
                String::new(),
                "mutation result exceeds the payload limit".to_string(),
                optimistic_present,
            ),
            Err(error) => (
                JetWebMutationStatus::Error,
                String::new(),
                jet_web_query_bound_error(error),
                optimistic_present,
            ),
        };
        if status == JetWebMutationStatus::Error {
            if rollback {
                self.set_state(previous.clone(), "optimistic-rollback");
                jet_web_query_publish_mutation(
                    &self.key,
                    JetDevtoolsMutationLifecycle::RolledBack,
                    true,
                );
            }
        } else {
            self.invalidate_targets(&targets);
            self.sync_live_state();
        }
        let value = self.state().value;
        let visible = JetWebMutationState {
            status,
            generation,
            optimistic: optimistic_present,
            value: value.clone(),
            result,
            error: error.clone(),
            context: context.clone(),
            rollback,
            paused: false,
            queued: 0,
            replayed: self.mutation.get().replayed,
        };
        self.set_mutation(visible);
        let settled = JetWebMutationState {
            status: JetWebMutationStatus::Settled,
            generation,
            optimistic: optimistic_present,
            value,
            result: self.mutation.get().result.clone(),
            error,
            context,
            rollback,
            paused: false,
            queued: 0,
            replayed: self.mutation.get().replayed,
        };
        self.set_mutation(settled.clone());
        settled
    }

    fn finish_mutation_error(
        &self,
        generation: u64,
        optimistic: bool,
        context: String,
        error: String,
        rollback: bool,
        queued: usize,
    ) -> JetWebMutationState {
        let error = jet_web_query_bound_error(error);
        let failed = JetWebMutationState {
            status: JetWebMutationStatus::Error,
            generation,
            optimistic,
            value: self.state().value,
            result: String::new(),
            error: error.clone(),
            context: context.clone(),
            rollback,
            paused: false,
            queued,
            replayed: self.mutation.get().replayed,
        };
        self.set_mutation(failed);
        jet_web_query_publish_mutation(&self.key, JetDevtoolsMutationLifecycle::Failed, rollback);
        let settled = JetWebMutationState {
            status: JetWebMutationStatus::Settled,
            generation,
            optimistic,
            value: self.state().value,
            result: String::new(),
            error,
            context,
            rollback,
            paused: false,
            queued,
            replayed: self.mutation.get().replayed,
        };
        self.set_mutation(settled.clone());
        settled
    }

    fn mutation_context(&self, before_generation: u64) -> String {
        let context = format!("query:{};before_generation:{before_generation}", self.key);
        if context.len() <= JET_WEB_QUERY_MAX_CONTEXT {
            context
        } else {
            "query mutation context exceeds the payload limit".to_string()
        }
    }

    pub fn get(&self) -> String {
        self.state().value
    }

    pub fn invalidate(&self) -> i64 {
        let before = self.state.get();
        let state = JetWebQueryState {
            status: if before.offline {
                JetWebQueryStatus::Offline
            } else {
                JetWebQueryStatus::Stale
            },
            error: String::new(),
            ..before
        };
        self.set_state(state, "manual-invalidate");
        let result = self
            .live
            .as_ref()
            .map(|live| jet_app_invalidate_key(live.key.clone()))
            .unwrap_or(0);
        self.sync_live_state();
        result
    }

    pub fn refresh(&self) -> Result<(), String> {
        let Some(fetch) = self.fetch.as_ref() else {
            return Err("query has no fetcher".to_string());
        };
        if !self.is_online() && self.mode() == JetWebQueryNetworkMode::OfflineFirst {
            let mut state = self.state.get();
            state.status = JetWebQueryStatus::Offline;
            state.offline = true;
            state.queued = jet_web_query_queue_count(&self.key);
            self.set_state(state, "refresh-paused");
            return Err("query refresh paused while offline".to_string());
        }
        let before = self.state.get();
        self.set_state(
            JetWebQueryState {
                status: JetWebQueryStatus::Fetching,
                error: String::new(),
                ..before
            },
            "refresh-start",
        );
        if self.live.is_some() {
            let _ = jet_app_live_refresh(self.live.as_ref().unwrap());
            let state = self.state();
            if state.status == JetWebQueryStatus::Error {
                return Err(state.error);
            }
            return Ok(());
        }
        match fetch() {
            Ok(value) if value.len() <= JET_WEB_QUERY_MAX_VALUE => {
                let before = self.state.get();
                self.set_state(
                    JetWebQueryState {
                        status: JetWebQueryStatus::Fresh,
                        value,
                        error: String::new(),
                        generation: before.generation.saturating_add(1),
                        ..before
                    },
                    "refresh-success",
                );
                Ok(())
            }
            Ok(_) => {
                let error = format!("query payload exceeds {} bytes", JET_WEB_QUERY_MAX_VALUE);
                self.set_query_error(error.clone(), "refresh-error");
                Err(error)
            }
            Err(error) => {
                let error = jet_web_query_bound_error(error);
                self.set_query_error(error.clone(), "refresh-error");
                Err(error)
            }
        }
    }

    pub fn cancel(&self) -> bool {
        let Some(live) = self.live.as_ref() else {
            return false;
        };
        let cancelled = jet_app_live_cancel(live);
        let state = self.state.get();
        self.set_state(
            JetWebQueryState {
                status: if state.offline {
                    JetWebQueryStatus::Offline
                } else {
                    JetWebQueryStatus::Stale
                },
                ..state
            },
            "cancelled",
        );
        cancelled.lifecycle.active
    }

    pub fn transact_invalidate(&self, write_set: String) -> i64 {
        let result = jet_app_transact_invalidate(write_set);
        self.sync_live_state();
        result
    }

    pub fn queue(&self, payload: String) -> Result<usize, String> {
        if payload.len() > JET_WEB_QUERY_MAX_VALUE {
            return Err(format!(
                "queued query payload exceeds {} bytes",
                JET_WEB_QUERY_MAX_VALUE
            ));
        }
        let count = jet_web_query_enqueue(&self.key, payload, Vec::new())?;
        let mut state = self.state.get();
        state.queued = count;
        state.offline = true;
        state.status = JetWebQueryStatus::Offline;
        state.error.clear();
        self.set_state(state, "queued");
        Ok(count)
    }

    fn retry_with<F>(&self, send: F) -> Result<usize, String>
    where
        F: Fn(String) -> Result<(), String>,
    {
        if !self.is_online() {
            return Err("query replay paused while offline".to_string());
        }
        let entries = jet_web_query_queued(&self.key)?;
        let mut sent = 0usize;
        for entry in entries {
            if let Err(error) = send(entry.payload.clone()) {
                let error = jet_web_query_bound_error(error);
                let mut mutation = self.mutation.get();
                mutation.status = JetWebMutationStatus::Error;
                mutation.error = error.clone();
                mutation.paused = true;
                mutation.queued = jet_web_query_queue_count(&self.key);
                self.set_mutation(mutation.clone());
                let mut settled = mutation;
                settled.status = JetWebMutationStatus::Settled;
                self.set_mutation(settled);
                let mut state = self.state.get();
                state.queued = jet_web_query_queue_count(&self.key);
                state.offline = true;
                state.status = JetWebQueryStatus::Offline;
                state.error = error.clone();
                self.set_state(state, "replay-error");
                return Err(error);
            }
            if let Err(error) = jet_web_query_remove_queued(&self.key, entry.sequence) {
                let error = jet_web_query_bound_error(error);
                let mut mutation = self.mutation.get();
                mutation.status = JetWebMutationStatus::Error;
                mutation.error = error.clone();
                mutation.paused = true;
                mutation.queued = jet_web_query_queue_count(&self.key);
                self.set_mutation(mutation.clone());
                let mut settled = mutation;
                settled.status = JetWebMutationStatus::Settled;
                self.set_mutation(settled);
                let mut state = self.state.get();
                state.queued = jet_web_query_queue_count(&self.key);
                state.offline = true;
                state.status = JetWebQueryStatus::Offline;
                state.error = error.clone();
                self.set_state(state, "replay-persist-error");
                return Err(error);
            }
            self.invalidate_targets(&entry.invalidations);
            sent = sent.saturating_add(1);
            let queued = jet_web_query_queue_count(&self.key);
            let mut mutation = self.mutation.get();
            mutation.status = JetWebMutationStatus::Success;
            mutation.paused = false;
            mutation.queued = queued;
            mutation.replayed = mutation.replayed.saturating_add(1);
            mutation.error.clear();
            self.set_mutation(mutation.clone());
            let mut settled = mutation;
            settled.status = JetWebMutationStatus::Settled;
            self.set_mutation(settled);
            let mut state = self.state.get();
            state.queued = queued;
            state.offline = queued > 0;
            state.status = if state.offline {
                JetWebQueryStatus::Offline
            } else {
                JetWebQueryStatus::Fresh
            };
            state.error.clear();
            self.set_state(state, "replayed");
        }
        Ok(sent)
    }

    pub fn show(&self) -> String {
        let state = self.state();
        let mutation = self.mutation_state();
        format!(
            "Query(key={},footprint={},status={},generation={},queued={},offline={},mode={},freshness_ms={},observers={},cause={},value_bytes={},error_bytes={},mutation={},rollback={},paused={},replayed={})",
            self.key,
            self.footprint,
            state.status.name(),
            state.generation,
            state.queued,
            state.offline,
            self.mode().name(),
            self.live
                .as_ref()
                .map(jet_app_live_freshness_ms)
                .unwrap_or(0),
            self.live
                .as_ref()
                .map(jet_app_live_observers)
                .unwrap_or(0),
            self.live
                .as_ref()
                .map(jet_app_live_invalidation_cause)
                .unwrap_or_else(|| "".to_string()),
            state.value.len(),
            state.error.len(),
            mutation.status.name(),
            mutation.rollback,
            mutation.paused,
            mutation.replayed,
        )
    }

    pub fn facts(&self) -> String {
        self.show()
    }

    fn set_state(&self, state: JetWebQueryState, cause: &str) {
        self.state.set(state.clone());
        self.publish_state(&state, cause);
    }

    fn set_query_error(&self, error: String, cause: &str) {
        let before = self.state.get();
        self.set_state(
            JetWebQueryState {
                status: JetWebQueryStatus::Error,
                error,
                ..before
            },
            cause,
        );
    }

    fn set_mutation(&self, state: JetWebMutationState) {
        self.mutation.set(state.clone());
        if state.status == JetWebMutationStatus::Pending {
            jet_web_query_publish_mutation(&self.key, JetDevtoolsMutationLifecycle::Started, state.rollback);
        } else if state.status == JetWebMutationStatus::Success {
            jet_web_query_publish_mutation(&self.key, JetDevtoolsMutationLifecycle::Committed, state.rollback);
        } else if state.status == JetWebMutationStatus::Error {
            jet_web_query_publish_mutation(&self.key, JetDevtoolsMutationLifecycle::Failed, state.rollback);
        }
    }

    fn publish_state(&self, state: &JetWebQueryState, cause: &str) {
        let observers = self
            .live
            .as_ref()
            .map(jet_app_live_observers)
            .unwrap_or(0);
        jet_web_query_publish_state(
            &self.key,
            &self.footprint,
            state,
            observers,
            cause,
            self.mutation.get().status,
            self.live
                .as_ref()
                .map(jet_app_live_freshness_ms)
                .unwrap_or(0),
        );
    }

    fn sync_live_state(&self) {
        let Some(live) = self.live.as_ref() else {
            return;
        };
        let live = jet_app_live_snapshot(live);
        let mut state = self.state.get();
        state.value = live.value.clone();
        state.generation = live.lifecycle.generation;
        state.queued = jet_web_query_queue_count(&self.key);
        if !live.lifecycle.error.is_empty() {
            state.status = JetWebQueryStatus::Error;
            state.error = live.lifecycle.error;
        } else if live.lifecycle.refreshing {
            state.status = JetWebQueryStatus::Fetching;
            state.error.clear();
        } else if live.lifecycle.dirty {
            state.status = if state.offline {
                JetWebQueryStatus::Offline
            } else {
                JetWebQueryStatus::Stale
            };
        } else if !state.offline && (state.status != JetWebQueryStatus::Pending || !live.value.is_empty()) {
            state.status = JetWebQueryStatus::Fresh;
            state.error.clear();
        }
        if state != self.state.get() {
            self.set_state(state, "live-sync");
        }
    }
}

fn jet_web_query_new(key: String, live: &JetLiveQuery) -> JetWebQuery {
    let snapshot = jet_app_live_snapshot(live);
    let mut query = JetWebQuery::new(key, snapshot.value);
    if !query.state.get().error.is_empty() {
        jet_web_query_register_panel_fact(&query);
        return query;
    }
    query.footprint = snapshot.footprint.display();
    query.fetch = snapshot.rerun;
    query.live = Some(query.bind_live_state(live));
    jet_web_query_register_panel_fact(&query);
    query
}

pub fn jet_web_query_live<F>(
    key: String,
    footprint: String,
    initial: String,
    fetch: F,
) -> JetWebQuery
where
    F: Fn() -> Result<String, String> + Send + Sync + 'static,
{
    JetWebQuery::live(key, footprint, initial, fetch)
}

pub fn jet_web_query_subscribe(source: String) -> JetWebQuery {
    JetWebQuery::subscribe(source)
}

pub fn jet_web_query_invalidate(query: &JetWebQuery) -> i64 {
    query.invalidate()
}

pub fn jet_web_query_get(query: &JetWebQuery) -> String {
    query.get()
}

pub fn jet_web_query_state(query: &JetWebQuery) -> JetWebQueryState {
    query.state()
}

pub fn jet_web_query_state_signal(
    query: &JetWebQuery,
) -> jet_std::JetSignal<JetWebQueryState> {
    query.state_signal()
}

pub fn jet_web_query_mutation_signal(
    query: &JetWebQuery,
) -> jet_std::JetSignal<JetWebMutationState> {
    query.mutation_signal()
}

pub fn jet_web_query_show(query: &JetWebQuery) -> String {
    query.show()
}

pub fn jet_web_query_facts(query: &JetWebQuery) -> String {
    query.facts()
}

pub fn jet_web_query_queue(query: &JetWebQuery, payload: String) -> Result<i64, String> {
    query.queue(payload).map(|count| count as i64)
}

pub fn jet_web_query_refresh(query: &JetWebQuery) -> Result<(), String> {
    query.refresh()
}

pub fn jet_web_query_cancel(query: &JetWebQuery) -> bool {
    query.cancel()
}

pub fn jet_web_query_set_online(query: &JetWebQuery, online: bool) {
    query.set_online(online);
}

pub fn jet_web_query_set_mode(query: &JetWebQuery, mode: JetWebQueryNetworkMode) {
    query.set_mode(mode);
}

pub fn jet_web_query_mutation_state(query: &JetWebQuery) -> JetWebMutationState {
    query.mutation_state()
}

/// Run one typed mutation through the canonical optimistic/offline lifecycle.
pub fn jet_web_query_mutate<F>(
    query: &JetWebQuery,
    payload: String,
    optimistic: Option<String>,
    action: F,
) -> JetWebMutationState
where
    F: Fn(String) -> Result<String, String>,
{
    query.mutate(payload, optimistic, action)
}

/// Run one mutation and invalidate the explicitly declared query/source keys
/// after a successful commit.
pub fn jet_web_query_mutate_with_invalidations<F>(
    query: &JetWebQuery,
    payload: String,
    optimistic: Option<String>,
    invalidations: Vec<String>,
    action: F,
) -> JetWebMutationState
where
    F: Fn(String) -> Result<String, String>,
{
    query.mutate_with_invalidations(payload, optimistic, invalidations, action)
}

/// Replay queued offline-first mutation payloads in deterministic sequence
/// order.  The sender owns the transport; queue removal and lifecycle state
/// remain in the canonical query implementation.
pub fn jet_web_query_retry<F>(
    query: &JetWebQuery,
    send: F,
) -> Result<i64, String>
where
    F: Fn(String) -> Result<(), String>,
{
    query.retry_with(send).map(|count| count as i64)
}
