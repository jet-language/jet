// D-LIVEQUERY1=A (#505): app.live / subscribe / invalidate.
// Engines marshal only; semantics live here (I9).

use std::collections::{BTreeMap as JetLiveBTreeMap, BTreeSet as JetLiveBTreeSet};
use std::sync::{Mutex as JetLiveMutex, OnceLock as JetLiveOnceLock};

const JET_LIVE_MAX_QUERIES: usize = 1024;
const JET_LIVE_ERR_INVALID_INPUT: i64 = -1;
const JET_LIVE_ERR_UNAVAILABLE: i64 = -2;

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

#[derive(Clone, Debug)]
struct JetLiveQuery {
    id: u64,
    footprint: JetLiveFootprint,
    value: String,
    generation: u64,
    active: bool,
    dirty: bool,
    error: String,
}

#[derive(Clone, Debug)]
struct JetLiveRecord {
    footprint: JetLiveFootprint,
    value: String,
    generation: u64,
    active: bool,
    dirty: bool,
    error: String,
}

#[derive(Default)]
struct JetLiveRegistry {
    queries: JetLiveBTreeMap<u64, JetLiveRecord>,
    next_id: u64,
    invalidations: u64,
    ws_pushes: u64,
    evictions: u64,
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
    }
}

fn jet_app_live(footprint: String, initial: String) -> JetLiveQuery {
    let Some(footprint) = JetLiveFootprint::parse(&footprint) else {
        return jet_live_error_query(0, JetLiveFootprint { paths: Vec::new() }, "invalid live footprint");
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
    };
    let query = jet_live_query(id, &record);
    state.queries.insert(id, record);
    query
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
    let mut hit = 0u64;
    for query in state.queries.values_mut() {
        if query.active && query.error.is_empty() && query.footprint.intersects(&footprint) {
            query.generation = query.generation.saturating_add(1);
            query.dirty = true;
            hit = hit.saturating_add(1);
        }
    }
    state.invalidations = state.invalidations.saturating_add(hit);
    hit.min(i64::MAX as u64) as i64
}

/// D-LIVEQUERY1: `#Transact` write-set → invalidate matching live footprints.
/// Invalidation never fabricates a result. A real rerun/push transport remains
/// an explicit core.ws/application-graph owner seam.
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
    updated.value = payload;
    updated.dirty = false;
    let result = jet_live_query(query.id, updated);
    state.ws_pushes = state.ws_pushes.saturating_add(1);
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
