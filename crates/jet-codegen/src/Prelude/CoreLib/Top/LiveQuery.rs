// D-LIVEQUERY1=A (#505): app.live / subscribe / invalidate.
// Engines marshal only; semantics live here (I9).

use std::collections::{BTreeMap as JetLiveBTreeMap, BTreeSet as JetLiveBTreeSet};
use std::sync::{Mutex as JetLiveMutex, OnceLock as JetLiveOnceLock};

/// A normalized read/write footprint.  The public Core API still accepts the
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
}

#[derive(Default)]
struct JetLiveRegistry {
    queries: JetLiveBTreeMap<u64, JetLiveQuery>,
    next_id: u64,
    invalidations: u64,
    ws_pushes: u64,
}

static JET_LIVE_REGISTRY: JetLiveOnceLock<JetLiveMutex<JetLiveRegistry>> = JetLiveOnceLock::new();

fn jet_live_registry() -> &'static JetLiveMutex<JetLiveRegistry> {
    JET_LIVE_REGISTRY.get_or_init(|| JetLiveMutex::new(JetLiveRegistry::default()))
}

fn jet_app_live(footprint: String, initial: String) -> JetLiveQuery {
    let Some(footprint) = JetLiveFootprint::parse(&footprint) else {
        return JetLiveQuery {
            id: 0,
            footprint: JetLiveFootprint { paths: Vec::new() },
            value: String::new(),
            generation: 0,
            active: false,
            dirty: false,
        };
    };
    let Ok(mut state) = jet_live_registry().lock() else {
        return JetLiveQuery {
            id: 0,
            footprint,
            value: String::new(),
            generation: 0,
            active: false,
            dirty: false,
        };
    };
    let Some(id) = state.next_id.checked_add(1) else {
        return JetLiveQuery {
            id: 0,
            footprint,
            value: String::new(),
            generation: 0,
            active: false,
            dirty: false,
        };
    };
    state.next_id = id;
    let query = JetLiveQuery {
        id,
        footprint,
        value: initial,
        generation: 1,
        active: true,
        dirty: false,
    };
    state.queries.insert(id, query.clone());
    query
}

fn jet_app_subscribe(source: String) -> JetLiveQuery {
    jet_app_live(format!("ext:{source}"), String::new())
}

fn jet_app_invalidate(footprint: String) -> i64 {
    let Some(footprint) = JetLiveFootprint::parse(&footprint) else {
        return 0;
    };
    let Ok(mut state) = jet_live_registry().lock() else {
        return 0;
    };
    let mut hit = 0u64;
    for query in state.queries.values_mut() {
        if query.active && query.footprint.intersects(&footprint) {
            query.generation = query.generation.saturating_add(1);
            query.dirty = true;
            hit = hit.saturating_add(1);
        }
    }
    state.invalidations = state.invalidations.saturating_add(hit);
    hit.min(i64::MAX as u64) as i64
}

/// D-LIVEQUERY1: `#Transact` write-set → invalidate matching live footprints.
/// A dirty query is not treated as a pushed result: only `signal_push` commits
/// a rerun payload and increments the `core.ws` receipt counter.
fn jet_app_transact_invalidate(write_set: String) -> i64 {
    let mut total = 0i64;
    for part in write_set.split(|c| c == ',' || c == ';' || c == ' ') {
        let footprint = part.trim();
        if !footprint.is_empty() {
            total = total.saturating_add(jet_app_invalidate(footprint.to_string()));
        }
    }
    total
}

fn jet_app_signal_push(query: &JetLiveQuery, payload: String) -> JetLiveQuery {
    let Ok(mut state) = jet_live_registry().lock() else {
        return query.clone();
    };
    let Some(updated) = state.queries.get_mut(&query.id) else {
        return query.clone();
    };
    if !updated.active {
        return updated.clone();
    }
    updated.generation = updated.generation.saturating_add(1);
    updated.value = payload;
    updated.dirty = false;
    let result = updated.clone();
    state.ws_pushes = state.ws_pushes.saturating_add(1);
    result
}

fn jet_app_live_get(query: &JetLiveQuery) -> String {
    let Ok(state) = jet_live_registry().lock() else {
        return String::new();
    };
    state
        .queries
        .get(&query.id)
        .filter(|stored| stored.active)
        .map(|stored| stored.value.clone())
        .unwrap_or_default()
}

fn jet_app_live_show(query: &JetLiveQuery) -> String {
    format!(
        "LiveQuery(id={}, footprint={}, generation={}, active={}, dirty={})",
        query.id,
        query.footprint.display(),
        query.generation,
        query.active,
        query.dirty
    )
}

fn jet_app_live_stats() -> String {
    let Ok(state) = jet_live_registry().lock() else {
        return "LiveStats(unavailable)".to_string();
    };
    format!(
        "LiveStats(queries={}, invalidations={}, ws_pushes={})",
        state.queries.len(),
        state.invalidations,
        state.ws_pushes
    )
}
