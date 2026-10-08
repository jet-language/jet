// Temporary ABI projections over existing host operations; no new runtime state.
// Replaced by Jet runtime layers L5/L6/L7/L9; every seed dies at cutover.

pub(crate) fn jet_log_record_emit_raw(level: &String, message: &String, fields: &Vec<jet_std::LogField>) {
    jet_log_emit(level.as_str(), message.as_str(), fields.as_slice());
}
pub(crate) fn jet_log_span_allocate_raw(name: &String) -> (i64, String) {
    let span = jet_ring_log_span(name);
    (span.id, span.name)
}
pub(crate) fn jet_browser_path_join_raw(root: &String, path: &String) -> String {
    std::path::Path::new(root).join(path).to_string_lossy().into_owned()
}
pub(crate) fn jet_browser_read_text_raw(path: &String) -> Option<String> {
    std::fs::read_to_string(path).ok()
}
pub(crate) fn jet_browser_is_file_raw(path: &String) -> bool {
    std::path::Path::new(path).is_file()
}
pub(crate) fn jet_browser_file_size_raw(path: &String) -> Option<i64> {
    std::fs::metadata(path).ok().map(|metadata| metadata.len() as i64)
}
pub(crate) fn jet_browser_modified_ms_raw(path: &String) -> Option<i64> {
    let metadata = std::fs::metadata(path).ok()?;
    Some(metadata.modified().ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0))
}
pub(crate) fn jet_openapi_contract_stop_raw(message: &String) -> ! {
    jet_runtime_stop("E2805", "", 0, message.as_str())
}
pub(crate) fn jet_live_stats_snapshot_raw() -> Option<(i64, i64, i64, u64, u64, u64)> {
    let state = jet_live_registry().lock().ok()?;
    Some((state.queries.len() as i64,
        state.queries.values().filter(|query| query.lifecycle.active).count() as i64,
        state.queries.values().filter(|query| query.lifecycle.active && query.lifecycle.refreshing).count() as i64,
        state.evictions, state.invalidations, state.ws_pushes))
}

pub(crate) fn jet_store_facts_snapshot_raw<T: Clone + Send + Sync + 'static>(store: &JetWebStore<T>) -> (u64, u64, i64, i64, i64, bool) {
    let _transaction_lock = store.transaction_lock.lock();
    let ledger = store.ledger.lock().unwrap_or_else(|error| error.into_inner());
    (ledger.current_generation, ledger.cursor, ledger.history.len() as i64,
        ledger.history_limit as i64, ledger.events.len() as i64,
        jet_web_store_history_capture_enabled())
}
pub(crate) fn jet_table_columns_raw<T: Clone + Send + Sync + 'static>(table: &JetWebTable<T>) -> Vec<JetWebTableColumn<T>> {
    (*table.columns).clone()
}
pub(crate) fn jet_table_replace_columns_raw<T: Clone + Send + Sync + 'static>(table: &JetWebTable<T>, columns: Vec<JetWebTableColumn<T>>) -> JetWebTable<T> {
    let mut result = table.clone();
    result.columns = JetWebTableArc::new(columns);
    result
}
pub(crate) fn jet_table_has_loader_raw<T>(table: &JetWebTable<T>) -> bool {
    table.server_page.is_some()
}
pub(crate) fn jet_table_invoke_loader_raw<T: Clone + Send + Sync + 'static>(table: &JetWebTable<T>, state: JetWebTableState) -> Result<JetWebTablePage<T>, String> {
    // Source tests presence before calling; this has no alternate loader policy.
    (table.server_page.as_ref().expect("checked table loader"))(state)
}
pub(crate) fn jet_table_validate_loaded_raw<T>(table: &JetWebTable<T>, page: JetWebTablePage<T>) -> Result<JetWebTablePage<T>, String> {
    jet_web_table_validate_server_page(page, table.row_key.as_ref())
}
pub(crate) fn jet_table_accept_keys_raw<T: Clone + Send + Sync + 'static>(table: &JetWebTable<T>, page: &JetWebTablePage<T>) -> Result<(), String> {
    table.accept_server_page_keys(page)
}
pub(crate) fn jet_table_loading_store_raw<T>(table: &JetWebTable<T>, value: bool) {
    table.loading.set(value);
}
pub(crate) fn jet_table_error_store_raw<T>(table: &JetWebTable<T>, value: String) {
    table.error.set(value);
}
pub(crate) fn jet_table_page_store_raw<T: Clone>(table: &JetWebTable<T>, page: Option<JetWebTablePage<T>>) {
    table.loaded_page.set(page);
}
pub(crate) fn jet_table_state_store_raw<T>(table: &JetWebTable<T>, state: JetWebTableState) {
    table.state.set(state);
}
