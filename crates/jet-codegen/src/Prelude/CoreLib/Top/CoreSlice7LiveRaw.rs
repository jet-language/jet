// Registry snapshot projection beside the existing LiveQuery state; no new state.
// Replaced by Jet runtime L9, dies at cutover.
pub(crate) fn jet_live_stats_snapshot_raw() -> Option<(i64, i64, i64, u64, u64, u64)> {
    let state = jet_live_registry().lock().ok()?;
    Some((state.queries.len() as i64,
        state.queries.values().filter(|query| query.lifecycle.active).count() as i64,
        state.queries.values().filter(|query| query.lifecycle.active && query.lifecycle.refreshing).count() as i64,
        state.evictions, state.invalidations, state.ws_pushes))
}
