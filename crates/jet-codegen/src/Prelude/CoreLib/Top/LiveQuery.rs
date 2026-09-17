// D-LIVEQUERY1=A (#505): app.live / subscribe / invalidate.
// Engines marshal only; semantics live here (I9).

use std::collections::{
    BTreeMap as JetLiveBTreeMap,
    BTreeSet as JetLiveBTreeSet,
    VecDeque as JetLiveVecDeque,
};
use std::time::{SystemTime as JetLiveSystemTime, UNIX_EPOCH as JetLiveUNIX_EPOCH};
use std::sync::{Arc as JetLiveArc, Mutex as JetLiveMutex, OnceLock as JetLiveOnceLock};

const JET_LIVE_MAX_QUERIES: usize = 1024;
const JET_LIVE_MAX_WS_SINKS: usize = 1024;
const JET_LIVE_MAX_PAYLOAD: usize = 4 * 1024 * 1024;
const JET_LIVE_MAX_TRANSPORT_EVENTS: usize = 2048;
const JET_LIVE_MAX_TRANSPORT_EVENT: usize = 1024 * 1024;
const JET_LIVE_MAX_REFRESHES_PER_INVALIDATION: usize = 8;
const JET_LIVE_ERR_INVALID_INPUT: i64 = -1;
const JET_LIVE_ERR_UNAVAILABLE: i64 = -2;

pub(crate) type JetLiveRerun = JetLiveArc<dyn Fn() -> Result<String, String> + Send + Sync + 'static>;
pub(crate) type JetLiveSink = JetLiveArc<dyn Fn(String) + Send + Sync + 'static>;

fn jet_live_now_ms() -> u64 {
    JetLiveSystemTime::now()
        .duration_since(JetLiveUNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

fn jet_live_age_ms(fresh_at_ms: u64) -> u64 {
    jet_live_now_ms().saturating_sub(fresh_at_ms)
}


fn jet_live_is_external(footprint: &JetLiveFootprint) -> bool {
    footprint.paths.iter().any(|path| path.starts_with("ext:"))
}

/// A normalized read/write footprint. The public Core API still accepts the
/// source spelling as a String, but the runtime never compares raw labels:
/// paths are tokenized, deduplicated, and matched by exact path or ancestry.
#[derive(Clone, Debug, PartialEq, Eq)]
struct JetLiveFootprint {
    paths: Vec<String>,
}

impl JetLiveFootprint {
    fn parse(source: &str) -> Option<Self> {
        let mut paths = JetLiveBTreeSet::new();
        for raw in source.split(|c| c == ',' || c == ';' || c == ' ') {
            let path = raw.trim();
            if path.is_empty()
                || path.len() > 512
                || path.chars().any(|c| {
                    c.is_control()
                        || !(c.is_ascii_alphanumeric()
                            || matches!(c, '_' | '-' | '.' | ':' | '/' | '[' | ']'))
                })
            {
                return None;
            }
            paths.insert(path.to_string());
        }
        (!paths.is_empty()).then(|| Self {
            paths: paths.into_iter().collect(),
        })
    }

    fn display(&self) -> String {
        self.paths.join(",")
    }

    fn intersects(&self, other: &Self) -> bool {
        self.paths.iter().any(|left| {
            other.paths.iter().any(|right| {
                left == right
                    || left
                        .strip_prefix(right)
                        .is_some_and(|rest| rest.starts_with('.'))
                    || right
                        .strip_prefix(left)
                        .is_some_and(|rest| rest.starts_with('.'))
            })
        })
    }
}

#[derive(Clone)]
pub(crate) struct JetLiveQuery {
    id: u64,
    key: String,
    footprint: JetLiveFootprint,
    value: String,
    pub(crate) lifecycle: JetLiveLifecycle,
    rerun: Option<JetLiveRerun>,
    sink: Option<JetLiveSink>,
}


impl std::fmt::Debug for JetLiveQuery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JetLiveQuery")
            .field("id", &self.id)
            .field("key", &self.key)
            .field("footprint", &self.footprint)
            .field("value", &self.value)
            .field("generation", &self.lifecycle.generation)
            .field("active", &self.lifecycle.active)
            .field("dirty", &self.lifecycle.dirty)
            .field("fresh_at_ms", &self.lifecycle.fresh_at_ms)
            .field("invalidation_cause", &self.lifecycle.invalidation_cause)
            .field("refreshing", &self.lifecycle.refreshing)
            .field("cancelled", &self.lifecycle.cancelled)
            .finish()
    }
}

#[derive(Clone)]
struct JetLiveRecord {
    key: String,
    footprint: JetLiveFootprint,
    value: String,
    lifecycle: JetLiveLifecycle,
    rerun: Option<JetLiveRerun>,
    sink: Option<JetLiveSink>,
}

#[derive(Default)]
struct JetLiveRegistry {
    queries: JetLiveBTreeMap<u64, JetLiveRecord>,
    next_id: u64,
    invalidations: u64,
    ws_pushes: u64,
    evictions: u64,
    ws_sinks: JetLiveBTreeMap<u64, JetLiveSink>,
    next_ws_sink_id: u64,
    transport_events: JetLiveBTreeMap<String, String>,
    transport_order: JetLiveVecDeque<String>,
}

static JET_LIVE_REGISTRY: JetLiveOnceLock<JetLiveMutex<JetLiveRegistry>> = JetLiveOnceLock::new();

fn jet_live_registry() -> &'static JetLiveMutex<JetLiveRegistry> {
    JET_LIVE_REGISTRY.get_or_init(|| JetLiveMutex::new(JetLiveRegistry::default()))
}

fn jet_live_error_query(
    id: u64,
    footprint: JetLiveFootprint,
    error: &str,
) -> JetLiveQuery {
    JetLiveQuery {
        id,
        key: String::new(),
        footprint,
        value: String::new(),
        lifecycle: JetLiveLifecycle::error(error),
        rerun: None,
        sink: None,
    }
}

fn jet_live_conflict_query(
    id: u64,
    key: String,
    requested: JetLiveFootprint,
    registered: &JetLiveFootprint,
) -> JetLiveQuery {
    // E2473 is the runtime counterpart of the sema key-identity rule:
    // one explicit key names one cache entry. A footprint is dependency
    // metadata, so a second declaration must agree rather than silently
    // creating a second cache entry.
    JetLiveQuery {
        id,
        key,
        footprint: requested.clone(),
        value: String::new(),
        lifecycle: JetLiveLifecycle::error(&format!(
            "E2473: query key is already registered with footprint `{}`; requested `{}`",
            registered.display(),
            requested.display(),
        )),
        rerun: None,
        sink: None,
    }
}

fn jet_live_query(id: u64, record: &JetLiveRecord) -> JetLiveQuery {
    JetLiveQuery {
        id,
        key: record.key.clone(),
        footprint: record.footprint.clone(),
        value: record.value.clone(),
        lifecycle: record.lifecycle.clone(),
        rerun: record.rerun.clone(),
        sink: record.sink.clone(),
    }
}

fn jet_live_payload(value: String) -> Result<String, String> {
    if value.len() > JET_LIVE_MAX_PAYLOAD {
        Err(format!(
            "live query payload exceeds {} bytes",
            JET_LIVE_MAX_PAYLOAD
        ))
    } else {
        Ok(value)
    }
}

fn jet_live_error_payload(error: String) -> String {
    if error.len() <= JET_LIVE_MAX_PAYLOAD {
        error
    } else {
        "live query callback error exceeds the payload limit".to_string()
    }
}

pub(crate) fn jet_app_live_keyed(
    key: String,
    footprint: String,
    initial: String,
    rerun: Option<JetLiveRerun>,
    sink: Option<JetLiveSink>,
) -> JetLiveQuery {
    let Some(footprint) = JetLiveFootprint::parse(&footprint) else {
        return jet_live_error_query(0, JetLiveFootprint { paths: Vec::new() }, "invalid live footprint");
    };
    let key = if key.trim().is_empty() {
        footprint.display()
    } else {
        key
    };
    if key.len() > 512 || key.chars().any(|character| character.is_control()) {
        return jet_live_error_query(0, footprint, "live query key is invalid");
    }
    let Ok(initial) = jet_live_payload(initial) else {
        return jet_live_error_query(0, footprint, "live query payload is too large");
    };
    let Ok(mut state) = jet_live_registry().lock() else {
        return jet_live_error_query(0, footprint, "live registry unavailable");
    };
    // Explicit keys are cache identity. Footprints are dependency metadata:
    // reusing a key with a different footprint is a checked conflict, never a
    // second cache entry that can drift from the first.
    if let Some(id) = state
        .queries
        .iter()
        .find(|(_, record)| record.lifecycle.active && record.key == key)
        .map(|(id, _)| *id)
    {
        let Some(record) = state.queries.get_mut(&id) else {
            return jet_live_error_query(0, footprint, "live registry entry disappeared");
        };
        if record.footprint != footprint {
            return jet_live_conflict_query(id, key, footprint, &record.footprint);
        }
        if record.rerun.is_none() {
            record.rerun = rerun;
        }
        if let Some(sink) = sink {
            record.sink = Some(sink);
        }
        return jet_live_query(id, record);
    }
    let Some(id) = state.next_id.checked_add(1) else {
        return jet_live_error_query(0, footprint, "live query id space exhausted");
    };
    state.next_id = id;
    if state.queries.len() >= JET_LIVE_MAX_QUERIES {
        if let Some(oldest) = state.queries.keys().next().copied() {
            state.queries.remove(&oldest);
            state.evictions = state.evictions.saturating_add(1);
        }
    }
    let now = jet_live_now_ms();
    let record = JetLiveRecord {
        key,
        footprint,
        value: initial,
        lifecycle: JetLiveLifecycle::active(now),
        rerun,
        sink,
    };
    let query = jet_live_query(id, &record);
    state.queries.insert(id, record);
    query
}

fn jet_app_live_with(
    footprint: String,
    initial: String,
    rerun: Option<JetLiveRerun>,
    sink: Option<JetLiveSink>,
) -> JetLiveQuery {
    jet_app_live_keyed(footprint.clone(), footprint, initial, rerun, sink)
}

fn jet_app_live(footprint: String, initial: String) -> JetLiveQuery {
    jet_app_live_with(footprint, initial, None, None)
}

/// D-LIVEQUERY1: the typed query runner is an engine-independent callback.
/// The callback is installed in the shared registry; invalidation calls it
/// outside the registry lock and publishes only the newest successful result.
fn jet_app_live_query<F>(footprint: String, initial: String, rerun: F) -> JetLiveQuery
where
    F: Fn() -> Result<String, String> + Send + Sync + 'static,
{
    jet_app_live_keyed_query(footprint.clone(), footprint, initial, rerun)
}

fn jet_app_live_keyed_query<F>(
    key: String,
    footprint: String,
    initial: String,
    rerun: F,
) -> JetLiveQuery
where
    F: Fn() -> Result<String, String> + Send + Sync + 'static,
{
    let query = jet_app_live_keyed(
        key,
        footprint,
        initial,
        Some(JetLiveArc::new(rerun)),
        None,
    );
    if query.id != 0 && query.lifecycle.error.is_empty() {
        jet_live_publish_ws(
            query.id,
            query.lifecycle.generation,
            &query.footprint,
            query.value.clone(),
        );
    }
    query
}

/// Bind a result sink to the canonical reactive delivery path. AOT uses this
/// with `jet_std::JetSignal`; the interpreter uses the same Prelude callback
/// without inventing a second signal carrier.
fn jet_app_live_bind_signal<F>(query: &JetLiveQuery, sink: F) -> JetLiveQuery
where
    F: Fn(String) + Send + Sync + 'static,
{
    jet_app_live_bind_sink(query, JetLiveArc::new(sink))
}

fn jet_app_live_bind_sink(query: &JetLiveQuery, sink: JetLiveSink) -> JetLiveQuery {
    let Ok(mut state) = jet_live_registry().lock() else {
        return jet_live_error_query(
            query.id,
            query.footprint.clone(),
            "live registry unavailable",
        );
    };
    let Some(record) = state.queries.get_mut(&query.id) else {
        return jet_live_error_query(query.id, query.footprint.clone(), "live query is closed");
    };
    if !record.lifecycle.active {
        return jet_live_error_query(query.id, record.footprint.clone(), "live query is closed");
    }
    record.sink = Some(sink);
    jet_live_query(query.id, record)
}
fn jet_app_live_snapshot(query: &JetLiveQuery) -> JetLiveQuery {
    let Ok(state) = jet_live_registry().lock() else {
        return jet_live_error_query(
            query.id,
            query.footprint.clone(),
            "live registry unavailable",
        );
    };
    state
        .queries
        .get(&query.id)
        .map(|record| jet_live_query(query.id, record))
        .unwrap_or_else(|| jet_live_error_query(query.id, query.footprint.clone(), "live query is closed"))
}
fn jet_app_live_refresh(query: &JetLiveQuery) -> i64 {
    let Ok(mut state) = jet_live_registry().lock() else {
        return JET_LIVE_ERR_UNAVAILABLE;
    };
    let mut reruns = Vec::new();
    let hit = jet_live_mark_id(
        &mut state,
        query.id,
        "manual-refresh",
        &mut reruns,
    );
    state.invalidations = state.invalidations.saturating_add(hit);
    drop(state);
    jet_live_run_refreshes(reruns);
    hit.min(i64::MAX as u64) as i64
}

fn jet_app_live_cancel(query: &JetLiveQuery) -> JetLiveQuery {
    if let Ok(mut state) = jet_live_registry().lock() {
        if let Some(record) = state.queries.get_mut(&query.id) {
            if record.lifecycle.cancel() {
                return jet_live_query(query.id, record);
            }
        }
    }
    jet_live_error_query(query.id, query.footprint.clone(), "live query is closed")
}

fn jet_app_live_close(query: &JetLiveQuery) -> i64 {
    let Ok(mut state) = jet_live_registry().lock() else {
        return JET_LIVE_ERR_UNAVAILABLE;
    };
    let Some(record) = state.queries.get_mut(&query.id) else {
        return 0;
    };
    record.lifecycle.close();
    1
}

fn jet_app_live_gc() -> i64 {
    let Ok(mut state) = jet_live_registry().lock() else {
        return JET_LIVE_ERR_UNAVAILABLE;
    };
    let before = state.queries.len();
    state.queries.retain(|_, query| query.lifecycle.active);
    let removed = before.saturating_sub(state.queries.len());
    state.evictions = state.evictions.saturating_add(removed as u64);
    removed.min(i64::MAX as usize) as i64
}

fn jet_app_live_observers(query: &JetLiveQuery) -> i64 {
    let Ok(state) = jet_live_registry().lock() else {
        return 0;
    };
    state
        .queries
        .get(&query.id)
        .map(|record| if record.sink.is_some() { 1 } else { 0 })
        .unwrap_or(0)
}

fn jet_app_live_freshness_ms(query: &JetLiveQuery) -> u64 {
    let Ok(state) = jet_live_registry().lock() else {
        return 0;
    };
    state
        .queries
        .get(&query.id)
        .map(|record| jet_live_age_ms(record.lifecycle.fresh_at_ms))
        .unwrap_or(0)
}

fn jet_app_live_invalidation_cause(query: &JetLiveQuery) -> String {
    let Ok(state) = jet_live_registry().lock() else {
        return "live registry unavailable".to_string();
    };
    state
        .queries
        .get(&query.id)
        .map(|record| record.lifecycle.invalidation_cause.clone())
        .unwrap_or_else(|| "live query is closed".to_string())
}

fn jet_app_live_is_external_query(query: &JetLiveQuery) -> bool {
    jet_live_is_external(&query.footprint)
}

/// Register one core.net.ws connection as a live transport. WebSocket writes are
/// supplied by the existing connection adapter; the live runtime owns only
/// bounded registration and event fan-out.
pub(crate) fn jet_app_ws_register(sink: JetLiveSink) -> u64 {
    let (id, replay) = {
        let Ok(mut state) = jet_live_registry().lock() else {
            return 0;
        };
        if state.ws_sinks.len() >= JET_LIVE_MAX_WS_SINKS {
            return 0;
        }
        let Some(id) = state.next_ws_sink_id.checked_add(1) else {
            return 0;
        };
        state.next_ws_sink_id = id;
        state.ws_sinks.insert(id, sink.clone());
        let replay = state
            .transport_order
            .iter()
            .filter_map(|topic| state.transport_events.get(topic).cloned())
            .collect::<Vec<_>>();
        (id, replay)
    };
    // Replay the last bounded event for each topic after releasing the
    // registry lock. This makes a reconnect converge to the current transport
    // state without retaining an unbounded operation log.
    for event in replay {
        sink(event);
    }
    id
}

pub(crate) fn jet_app_ws_unregister(id: u64) {
    if id == 0 {
        return;
    }
    if let Ok(mut state) = jet_live_registry().lock() {
        state.ws_sinks.remove(&id);
    }
}

pub(crate) fn jet_live_publish_transport(topic: String, event: String) {
    if topic.is_empty()
        || topic.len() > 512
        || event.is_empty()
        || event.len() > JET_LIVE_MAX_TRANSPORT_EVENT
    {
        return;
    }
    let sinks = {
        let Ok(mut state) = jet_live_registry().lock() else {
            return;
        };
        state.transport_events.insert(topic.clone(), event.clone());
        state.transport_order.retain(|existing| existing != &topic);
        state.transport_order.push_back(topic);
        while state.transport_order.len() > JET_LIVE_MAX_TRANSPORT_EVENTS {
            let Some(oldest) = state.transport_order.pop_front() else {
                break;
            };
            state.transport_events.remove(&oldest);
        }
        state.ws_pushes = state.ws_pushes.saturating_add(1);
        state.ws_sinks.values().cloned().collect::<Vec<_>>()
    };
    for sink in sinks {
        sink(event.clone());
    }
}

fn jet_live_publish_ws(
    id: u64,
    generation: u64,
    footprint: &JetLiveFootprint,
    value: String,
) {
    jet_live_publish_transport(
        format!("live:{id}"),
        format!(
            "live:{}:{}:{}:{}",
            id,
            generation,
            footprint.display(),
            value
        ),
    );
}

fn jet_app_subscribe(source: String) -> JetLiveQuery {
    if source.trim().is_empty()
        || source.len() > 512
        || source.chars().any(|character| character.is_control())
    {
        return jet_live_error_query(
            0,
            JetLiveFootprint { paths: Vec::new() },
            "subscription source is invalid",
        );
    }
    jet_app_live(format!("ext:{source}"), String::new())
}

fn jet_live_schedule_latest(
    state: &mut JetLiveRegistry,
    id: u64,
    reruns: &mut Vec<(u64, u64, JetLiveRerun)>,
) {
    let Some(query) = state.queries.get_mut(&id) else {
        return;
    };
    if !query.lifecycle.active
        || query.lifecycle.cancelled
        || !query.lifecycle.dirty
        || query.lifecycle.refreshing
    {
        return;
    }
    let Some(rerun) = query.rerun.clone() else {
        return;
    };
    let generation = query.lifecycle.generation;
    if !query.lifecycle.begin_refresh(generation) {
        return;
    }
    reruns.push((id, generation, rerun));
}

fn jet_live_mark_footprint(
    state: &mut JetLiveRegistry,
    footprint: &JetLiveFootprint,
    cause: &str,
    reruns: &mut Vec<(u64, u64, JetLiveRerun)>,
) -> u64 {
    let mut hit = 0u64;
    let ids = state
        .queries
        .iter()
        .filter(|(_, query)| query.lifecycle.active && query.footprint.intersects(footprint))
        .map(|(id, _)| *id)
        .collect::<Vec<_>>();
    for id in ids {
        let schedule = if let Some(query) = state.queries.get_mut(&id) {
            let was_refreshing = query.lifecycle.refreshing;
            if query.lifecycle.invalidate(cause) {
                hit = hit.saturating_add(1);
                !was_refreshing
            } else {
                false
            }
        } else {
            false
        };
        if schedule {
            jet_live_schedule_latest(state, id, reruns);
        }
    }
    hit
}

fn jet_live_mark_key(
    state: &mut JetLiveRegistry,
    key: &str,
    cause: &str,
    reruns: &mut Vec<(u64, u64, JetLiveRerun)>,
) -> u64 {
    let mut hit = 0u64;
    let ids = state
        .queries
        .iter()
        .filter(|(_, query)| query.lifecycle.active && query.key == key)
        .map(|(id, _)| *id)
        .collect::<Vec<_>>();
    for id in ids {
        let schedule = if let Some(query) = state.queries.get_mut(&id) {
            let was_refreshing = query.lifecycle.refreshing;
            if query.lifecycle.invalidate(cause) {
                hit = hit.saturating_add(1);
                !was_refreshing
            } else {
                false
            }
        } else {
            false
        };
        if schedule {
            jet_live_schedule_latest(state, id, reruns);
        }
    }
    hit
}

fn jet_live_mark_id(
    state: &mut JetLiveRegistry,
    id: u64,
    cause: &str,
    reruns: &mut Vec<(u64, u64, JetLiveRerun)>,
) -> u64 {
    let schedule = if let Some(query) = state.queries.get_mut(&id) {
        if !query.lifecycle.active {
            return 0;
        }
        let was_refreshing = query.lifecycle.refreshing;
        query.lifecycle.invalidate(cause);
        !was_refreshing
    } else {
        return 0;
    };
    if schedule {
        jet_live_schedule_latest(state, id, reruns);
    }
    1
}

fn jet_live_run_refreshes(mut reruns: Vec<(u64, u64, JetLiveRerun)>) {
    let mut runs = 0usize;
    while let Some((id, generation, rerun)) = reruns.pop() {
        if runs >= JET_LIVE_MAX_REFRESHES_PER_INVALIDATION {
            if let Ok(mut state) = jet_live_registry().lock() {
                if let Some(query) = state.queries.get_mut(&id) {
                    query.lifecycle.refreshing = false;
                }
                for (pending_id, _, _) in reruns.drain(..) {
                    if let Some(query) = state.queries.get_mut(&pending_id) {
                        query.lifecycle.refreshing = false;
                    }
                }
            }
            break;
        }
        runs += 1;
        let result = rerun();
        let mut delivery: Option<(JetLiveQuery, Option<JetLiveSink>, String)> = None;
        let mut next = None;
        if let Ok(mut state) = jet_live_registry().lock() {
            let schedule_latest = if let Some(query) = state.queries.get_mut(&id) {
                let mut schedule = false;
                match result {
                    Ok(value) => match jet_live_payload(value) {
                        Ok(value) => {
                            if query.lifecycle.is_current(generation) {
                                query.value = value.clone();
                                let _ = query.lifecycle.publish(generation, jet_live_now_ms());
                                delivery = Some((
                                    jet_live_query(id, query),
                                    query.sink.clone(),
                                    value,
                                ));
                            } else {
                                let _ = query.lifecycle.publish(generation, jet_live_now_ms());
                                schedule = true;
                            }
                        }
                        Err(_) if query.lifecycle.is_current(generation) => {
                            let _ = query.lifecycle.fail(
                                generation,
                                "live query callback result exceeds the payload limit".to_string(),
                            );
                        }
                        Err(_) => {
                            let _ = query.lifecycle.publish(generation, jet_live_now_ms());
                            schedule = true;
                        }
                    },
                    Err(error) => {
                        if !query
                            .lifecycle
                            .fail(generation, jet_live_error_payload(error))
                        {
                            schedule = true;
                        }
                    }
                }
                schedule
            } else {
                false
            };
            if schedule_latest {
                let mut pending = Vec::new();
                jet_live_schedule_latest(&mut state, id, &mut pending);
                next = pending.pop();
            }
        }
        if let Some((updated, sink, value)) = delivery {
            if let Some(sink) = sink {
                sink(value.clone());
            }
            jet_live_publish_ws(
                updated.id,
                updated.lifecycle.generation,
                &updated.footprint,
                value,
            );
        }
        if let Some(next) = next {
            reruns.push(next);
        }
    }
}

fn jet_app_invalidate_with_cause(footprint: String, cause: String) -> i64 {
    let Some(footprint) = JetLiveFootprint::parse(&footprint) else {
        return JET_LIVE_ERR_INVALID_INPUT;
    };
    let Ok(mut state) = jet_live_registry().lock() else {
        return JET_LIVE_ERR_UNAVAILABLE;
    };
    let mut reruns = Vec::new();
    let hit = jet_live_mark_footprint(&mut state, &footprint, &cause, &mut reruns);
    state.invalidations = state.invalidations.saturating_add(hit);
    drop(state);
    jet_live_run_refreshes(reruns);
    hit.min(i64::MAX as u64) as i64
}

fn jet_app_invalidate(footprint: String) -> i64 {
    let cause = format!("footprint:{footprint}");
    jet_app_invalidate_with_cause(footprint, cause)
}

pub(crate) fn jet_app_invalidate_key(key: String) -> i64 {
    if key.trim().is_empty()
        || key.len() > 512
        || key.chars().any(|character| character.is_control())
    {
        return JET_LIVE_ERR_INVALID_INPUT;
    }
    let Ok(mut state) = jet_live_registry().lock() else {
        return JET_LIVE_ERR_UNAVAILABLE;
    };
    let mut reruns = Vec::new();
    let hit = jet_live_mark_key(
        &mut state,
        &key,
        &format!("key:{key}"),
        &mut reruns,
    );
    state.invalidations = state.invalidations.saturating_add(hit);
    drop(state);
    jet_live_run_refreshes(reruns);
    hit.min(i64::MAX as u64) as i64
}


/// D-LIVEQUERY1: `#Transact` write-set → invalidate matching live footprints.
/// Invalidation never fabricates a result. Registered typed rerunners execute
/// outside the registry lock; successful results update the canonical query,
/// signal sinks, and existing core.net.ws transport.
fn jet_app_transact_invalidate(write_set: String) -> i64 {
    if write_set.trim().is_empty() {
        return JET_LIVE_ERR_INVALID_INPUT;
    }
    let mut total = 0i64;
    for part in write_set.split(|c| c == ',' || c == ';' || c == ' ') {
        let footprint = part.trim();
        if !footprint.is_empty() {
            let hit = jet_app_invalidate_with_cause(
                footprint.to_string(),
                format!("transaction:{footprint}"),
            );
            if hit < 0 {
                return hit;
            }
            total = total.saturating_add(hit);
        }
    }
    total
}

fn jet_app_signal_push(query: &JetLiveQuery, payload: String) -> JetLiveQuery {
    let Ok(payload) = jet_live_payload(payload) else {
        return jet_live_error_query(query.id, query.footprint.clone(), "live query payload is too large");
    };
    let Ok(mut state) = jet_live_registry().lock() else {
        return jet_live_error_query(query.id, query.footprint.clone(), "live registry unavailable");
    };
    let Some(updated) = state.queries.get_mut(&query.id) else {
        return jet_live_error_query(query.id, query.footprint.clone(), "live query is closed");
    };
    if !updated.lifecycle.active {
        return jet_live_error_query(query.id, updated.footprint.clone(), "live query is closed");
    }
    let _ = updated.lifecycle.invalidate("signal-push");
    updated.value = payload.clone();
    let generation = updated.lifecycle.generation;
    let _ = updated.lifecycle.publish(generation, jet_live_now_ms());
    updated.lifecycle.invalidation_cause = "signal-push".to_string();
    let sink = updated.sink.clone();
    let result = jet_live_query(query.id, updated);
    drop(state);
    if let Some(sink) = sink {
        sink(payload.clone());
    }
    jet_live_publish_ws(
        result.id,
        result.lifecycle.generation,
        &result.footprint,
        payload,
    );
    result
}

fn jet_app_live_get(query: &JetLiveQuery) -> String {
    let Ok(state) = jet_live_registry().lock() else {
        return "LiveError(live registry unavailable)".to_string();
    };
    let Some(stored) = state.queries.get(&query.id) else {
        return "LiveError(live query is closed)".to_string();
    };
    if !stored.lifecycle.active {
        return "LiveError(live query is closed)".to_string();
    }
    if !stored.lifecycle.error.is_empty() {
        return "LiveError(live query callback failed)".to_string();
    }
    stored.value.clone()
}

fn jet_app_live_show(query: &JetLiveQuery) -> String {
    let Ok(state) = jet_live_registry().lock() else {
        return format!("LiveQueryError(id={}, reason=live registry unavailable)", query.id);
    };
    let Some(stored) = state.queries.get(&query.id) else {
        return format!("LiveQueryError(id={}, reason=live query is closed)", query.id);
    };
    let source = if jet_live_is_external(&stored.footprint) {
        "external"
    } else {
        "database"
    };
    format!(
        "LiveQuery(id={},key={},source={},footprint={},generation={},active={},dirty={},refreshing={},observers={},freshness_ms={},cause={},value_bytes={},error_bytes={})",
        query.id,
        stored.key,
        source,
        stored.footprint.display(),
        stored.lifecycle.generation,
        stored.lifecycle.active,
        stored.lifecycle.dirty,
        stored.lifecycle.refreshing,
        if stored.sink.is_some() { 1 } else { 0 },
        jet_live_age_ms(stored.lifecycle.fresh_at_ms),
        stored.lifecycle.invalidation_cause,
        stored.value.len(),
        stored.lifecycle.error.len()
    )
}

fn jet_app_live_stats() -> String {
    let Ok(state) = jet_live_registry().lock() else {
        return "LiveStats(error=live registry unavailable)".to_string();
    };
    let active = state
        .queries
        .values()
        .filter(|query| query.lifecycle.active)
        .count();
    let refreshing = state
        .queries
        .values()
        .filter(|query| query.lifecycle.active && query.lifecycle.refreshing)
        .count();
    format!(
        "LiveStats(queries={},active={},refreshing={},limit={},evictions={},invalidations={},ws_pushes={})",
        state.queries.len(),
        active,
        refreshing,
        JET_LIVE_MAX_QUERIES,
        state.evictions,
        state.invalidations,
        state.ws_pushes
    )
}
