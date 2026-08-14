// D-LIVEQUERY1=A (#505): app.live / subscribe / invalidate.
// Engines marshal only; semantics live here (I9).

use std::collections::{
    BTreeMap as JetLiveBTreeMap,
    BTreeSet as JetLiveBTreeSet,
    VecDeque as JetLiveVecDeque,
};
use std::sync::{Arc as JetLiveArc, Mutex as JetLiveMutex, OnceLock as JetLiveOnceLock};

const JET_LIVE_MAX_QUERIES: usize = 1024;
const JET_LIVE_MAX_WS_SINKS: usize = 1024;
const JET_LIVE_MAX_PAYLOAD: usize = 4 * 1024 * 1024;
const JET_LIVE_MAX_TRANSPORT_EVENTS: usize = 2048;
const JET_LIVE_MAX_TRANSPORT_EVENT: usize = 1024 * 1024;
const JET_LIVE_ERR_INVALID_INPUT: i64 = -1;
const JET_LIVE_ERR_UNAVAILABLE: i64 = -2;

type JetLiveRerun = JetLiveArc<dyn Fn() -> Result<String, String> + Send + Sync + 'static>;
pub(crate) type JetLiveSink = JetLiveArc<dyn Fn(String) + Send + Sync + 'static>;

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
struct JetLiveQuery {
    id: u64,
    footprint: JetLiveFootprint,
    value: String,
    generation: u64,
    active: bool,
    dirty: bool,
    error: String,
    rerun: Option<JetLiveRerun>,
    sink: Option<JetLiveSink>,
}

impl std::fmt::Debug for JetLiveQuery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JetLiveQuery")
            .field("id", &self.id)
            .field("footprint", &self.footprint)
            .field("value", &self.value)
            .field("generation", &self.generation)
            .field("active", &self.active)
            .field("dirty", &self.dirty)
            .field("error", &self.error)
            .finish()
    }
}

#[derive(Clone)]
struct JetLiveRecord {
    footprint: JetLiveFootprint,
    value: String,
    generation: u64,
    active: bool,
    dirty: bool,
    error: String,
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
        footprint,
        value: String::new(),
        generation: 0,
        active: false,
        dirty: false,
        error: error.to_string(),
        rerun: None,
        sink: None,
    }
}

fn jet_live_query(id: u64, record: &JetLiveRecord) -> JetLiveQuery {
    JetLiveQuery {
        id,
        footprint: record.footprint.clone(),
        value: record.value.clone(),
        generation: record.generation,
        active: record.active,
        dirty: record.dirty,
        error: record.error.clone(),
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

fn jet_app_live_with(
    footprint: String,
    initial: String,
    rerun: Option<JetLiveRerun>,
    sink: Option<JetLiveSink>,
) -> JetLiveQuery {
    let Some(footprint) = JetLiveFootprint::parse(&footprint) else {
        return jet_live_error_query(0, JetLiveFootprint { paths: Vec::new() }, "invalid live footprint");
    };
    let Ok(initial) = jet_live_payload(initial) else {
        return jet_live_error_query(0, footprint, "live query payload is too large");
    };
    let Ok(mut state) = jet_live_registry().lock() else {
        return jet_live_error_query(0, footprint, "live registry unavailable");
    };
    let Some(id) = state.next_id.checked_add(1) else {
        return jet_live_error_query(0, footprint, "live query id space exhausted");
    };
    if state.queries.len() >= JET_LIVE_MAX_QUERIES {
        // Bounded registry: remove oldest state. Existing handles become closed
        // and report an error instead of reading a stale local copy.
        if let Some(oldest) = state.queries.keys().next().copied() {
            state.queries.remove(&oldest);
            state.evictions = state.evictions.saturating_add(1);
        }
    }
    state.next_id = id;
    let record = JetLiveRecord {
        footprint,
        value: initial,
        generation: 1,
        active: true,
        dirty: false,
        error: String::new(),
        rerun,
        sink,
    };
    let query = jet_live_query(id, &record);
    state.queries.insert(id, record);
    query
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
    let query = jet_app_live_with(footprint, initial, Some(JetLiveArc::new(rerun)), None);
    if query.id != 0 && query.error.is_empty() {
        // Seed the bounded transport with the current value. A connection
        // opened after the query is created must receive the same snapshot as
        // a connection that was already present.
        jet_live_publish_ws(query.id, query.generation, &query.footprint, query.value.clone());
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
    if !record.active || !record.error.is_empty() {
        return jet_live_error_query(query.id, record.footprint.clone(), "live query is closed");
    }
    record.sink = Some(sink);
    jet_live_query(query.id, record)
}

/// Register one core.ws connection as a live transport. WebSocket writes are
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
    if source.trim().is_empty() {
        return jet_live_error_query(0, JetLiveFootprint { paths: Vec::new() }, "subscription source is empty");
    }
    jet_app_live(format!("ext:{source}"), String::new())
}

fn jet_app_invalidate(footprint: String) -> i64 {
    let Some(footprint) = JetLiveFootprint::parse(&footprint) else {
        return JET_LIVE_ERR_INVALID_INPUT;
    };
    let Ok(mut state) = jet_live_registry().lock() else {
        return JET_LIVE_ERR_UNAVAILABLE;
    };
    let mut reruns = Vec::new();
    let mut hit = 0u64;
    for (id, query) in state.queries.iter_mut() {
        if query.active && query.error.is_empty() && query.footprint.intersects(&footprint) {
            query.generation = query.generation.saturating_add(1);
            query.dirty = true;
            hit = hit.saturating_add(1);
            if let Some(rerun) = query.rerun.clone() {
                reruns.push((*id, query.generation, query.footprint.clone(), rerun, query.sink.clone()));
            }
        }
    }
    state.invalidations = state.invalidations.saturating_add(hit);
    drop(state);

    // A query body may itself touch the live registry. Never execute user
    // callbacks while holding the registry mutex.
    for (id, generation, _footprint, rerun, sink) in reruns {
        match rerun() {
            Ok(value) => match jet_live_payload(value) {
                Ok(value) => {
                    let mut publish = None;
                    if let Ok(mut state) = jet_live_registry().lock() {
                        let updated = match state.queries.get_mut(&id) {
                            Some(query)
                                if query.active
                                    && query.error.is_empty()
                                    && query.generation == generation =>
                            {
                                query.value = value.clone();
                                query.dirty = false;
                                Some(jet_live_query(id, query))
                            }
                            _ => None,
                        };
                        publish = updated;
                    }
                    if let Some(updated) = publish {
                        if let Some(sink) = sink {
                            sink(value.clone());
                        }
                        jet_live_publish_ws(
                            updated.id,
                            updated.generation,
                            &updated.footprint,
                            value,
                        );
                    }
                }
                Err(error) => {
                    if let Ok(mut state) = jet_live_registry().lock() {
                        if let Some(query) = state.queries.get_mut(&id) {
                            if query.active && query.generation == generation {
                                query.error = error;
                                query.dirty = true;
                            }
                        }
                    }
                }
            },
            Err(error) => {
                if let Ok(mut state) = jet_live_registry().lock() {
                    if let Some(query) = state.queries.get_mut(&id) {
                        if query.active && query.generation == generation {
                            query.error = error;
                            query.dirty = true;
                        }
                    }
                }
            }
        }
    }
    hit.min(i64::MAX as u64) as i64
}

/// D-LIVEQUERY1: `#Transact` write-set → invalidate matching live footprints.
/// Invalidation never fabricates a result. Registered typed rerunners execute
/// outside the registry lock; successful results update the canonical query,
/// signal sink, and existing core.net.ws transport.
fn jet_app_transact_invalidate(write_set: String) -> i64 {
    if write_set.trim().is_empty() {
        return JET_LIVE_ERR_INVALID_INPUT;
    }
    let mut total = 0i64;
    for part in write_set.split(|c| c == ',' || c == ';' || c == ' ') {
        let footprint = part.trim();
        if !footprint.is_empty() {
            let hit = jet_app_invalidate(footprint.to_string());
            if hit < 0 {
                return hit;
            }
            total = total.saturating_add(hit);
        }
    }
    total
}

fn jet_app_signal_push(query: &JetLiveQuery, payload: String) -> JetLiveQuery {
    if !query.error.is_empty() {
        return query.clone();
    }
    let Ok(payload) = jet_live_payload(payload) else {
        return jet_live_error_query(query.id, query.footprint.clone(), "live query payload is too large");
    };
    let Ok(mut state) = jet_live_registry().lock() else {
        return jet_live_error_query(query.id, query.footprint.clone(), "live registry unavailable");
    };
    let Some(updated) = state.queries.get_mut(&query.id) else {
        return jet_live_error_query(query.id, query.footprint.clone(), "live query is closed");
    };
    if !updated.active || !updated.error.is_empty() {
        return jet_live_error_query(query.id, updated.footprint.clone(), "live query is closed");
    }
    updated.generation = updated.generation.saturating_add(1);
    updated.value = payload.clone();
    updated.dirty = false;
    let result = jet_live_query(query.id, updated);
    let sink = result.sink.clone();
    drop(state);
    if let Some(sink) = sink {
        sink(payload.clone());
    }
    jet_live_publish_ws(result.id, result.generation, &result.footprint, payload);
    result
}

fn jet_app_live_get(query: &JetLiveQuery) -> String {
    if !query.error.is_empty() {
        return format!("LiveError({})", query.error);
    }
    let Ok(state) = jet_live_registry().lock() else {
        return "LiveError(live registry unavailable)".to_string();
    };
    let Some(stored) = state.queries.get(&query.id) else {
        return "LiveError(live query is closed)".to_string();
    };
    if !stored.active || !stored.error.is_empty() {
        return format!("LiveError({})", if stored.error.is_empty() { "live query is closed" } else { &stored.error });
    }
    stored.value.clone()
}

fn jet_app_live_show(query: &JetLiveQuery) -> String {
    if !query.error.is_empty() {
        return format!("LiveQueryError(id={}, reason={})", query.id, query.error);
    }
    let Ok(state) = jet_live_registry().lock() else {
        return format!("LiveQueryError(id={}, reason=live registry unavailable)", query.id);
    };
    let Some(stored) = state.queries.get(&query.id) else {
        return format!("LiveQueryError(id={}, reason=live query is closed)", query.id);
    };
    format!(
        "LiveQuery(id={}, footprint={}, generation={}, active={}, dirty={}, error={})",
        query.id,
        stored.footprint.display(),
        stored.generation,
        stored.active,
        stored.dirty,
        stored.error
    )
}

fn jet_app_live_stats() -> String {
    let Ok(state) = jet_live_registry().lock() else {
        return "LiveStats(error=live registry unavailable)".to_string();
    };
    format!(
        "LiveStats(queries={}, limit={}, evictions={}, invalidations={}, ws_pushes={})",
        state.queries.len(),
        JET_LIVE_MAX_QUERIES,
        state.evictions,
        state.invalidations,
        state.ws_pushes
    )
}
