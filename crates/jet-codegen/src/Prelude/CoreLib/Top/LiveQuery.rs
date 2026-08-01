// D-LIVEQUERY1=A (#505): app.live / subscribe / invalidate.
// Engines marshal only; semantics live here (I9).

#[derive(Clone, Debug)]
struct JetLiveQuery {
    id: String,
    footprint: String,
    value: String,
    generation: i64,
    active: bool,
}

#[derive(Default)]
struct JetLiveRegistry {
    queries: Vec<JetLiveQuery>,
    next_id: i64,
    invalidations: i64,
    ws_pushes: i64,
}

thread_local! {
    static JET_LIVE_REGISTRY: std::cell::RefCell<JetLiveRegistry> =
        std::cell::RefCell::new(JetLiveRegistry::default());
}

fn jet_app_live(footprint: String, initial: String) -> JetLiveQuery {
    JET_LIVE_REGISTRY.with(|reg| {
        let mut state = reg.borrow_mut();
        state.next_id += 1;
        let q = JetLiveQuery {
            id: format!("lq-{}", state.next_id),
            footprint,
            value: initial,
            generation: 1,
            active: true,
        };
        state.queries.push(q.clone());
        q
    })
}

fn jet_app_subscribe(source: String) -> JetLiveQuery {
    jet_app_live(format!("ext:{source}"), String::new())
}

fn jet_app_invalidate(footprint: String) -> i64 {
    JET_LIVE_REGISTRY.with(|reg| {
        let mut state = reg.borrow_mut();
        let mut hit = 0i64;
        for q in &mut state.queries {
            if q.active && q.footprint == footprint {
                q.generation += 1;
                q.value = format!("invalidated@g{}", q.generation);
                hit += 1;
            }
        }
        state.ws_pushes += hit;
        state.invalidations += hit;
        hit
    })
}

/// D-LIVEQUERY1: `#Transact` write-set → invalidate matching live footprints
/// and count a `core.ws` push into the Signal payload generation.
fn jet_app_transact_invalidate(write_set: String) -> i64 {
    let mut total = 0i64;
    for part in write_set.split(|c| c == ',' || c == ';' || c == ' ') {
        let footprint = part.trim();
        if !footprint.is_empty() {
            total += jet_app_invalidate(footprint.to_string());
        }
    }
    total
}

fn jet_app_signal_push(query: &JetLiveQuery, payload: String) -> JetLiveQuery {
    JET_LIVE_REGISTRY.with(|reg| {
        let mut state = reg.borrow_mut();
        let found = state
            .queries
            .iter_mut()
            .find(|q| q.id == query.id)
            .map(|q| {
                q.generation += 1;
                q.value = payload.clone();
                q.clone()
            });
        if let Some(updated) = found {
            state.ws_pushes += 1;
            updated
        } else {
            query.clone()
        }
    })
}

fn jet_app_live_get(query: &JetLiveQuery) -> String {
    JET_LIVE_REGISTRY.with(|reg| {
        let state = reg.borrow();
        state
            .queries
            .iter()
            .find(|q| q.id == query.id)
            .map(|q| q.value.clone())
            .unwrap_or_else(|| query.value.clone())
    })
}

fn jet_app_live_show(query: &JetLiveQuery) -> String {
    format!(
        "LiveQuery(id={}, footprint={}, generation={}, active={})",
        query.id, query.footprint, query.generation, query.active
    )
}

fn jet_app_live_stats() -> String {
    JET_LIVE_REGISTRY.with(|reg| {
        let state = reg.borrow();
        format!(
            "LiveStats(queries={}, invalidations={}, ws_pushes={})",
            state.queries.len(),
            state.invalidations,
            state.ws_pushes
        )
    })
}
