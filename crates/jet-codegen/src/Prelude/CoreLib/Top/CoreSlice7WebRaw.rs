// Temporary ABI projections over existing host operations; no new runtime state.
// Replaced by Jet runtime layers L5/L6/L7/L9; every seed dies at cutover.

pub(crate) fn jet_openapi_contract_stop_raw(message: &String) -> ! {
    jet_runtime_stop("E2805", "", 0, message.as_str())
}

pub(crate) fn jet_store_facts_snapshot_raw<T: Clone + Send + Sync + 'static>(store: &JetWebStore<T>) -> (u64, u64, i64, i64, i64, bool) {
    let _transaction_lock = store.transaction_lock.lock();
    let ledger = store.ledger.lock().unwrap_or_else(|error| error.into_inner());
    (ledger.current_generation, ledger.cursor as u64, ledger.history.len() as i64,
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
pub(crate) fn jet_table_commit_keys_raw<T, F>(table: &JetWebTable<T>, page: &JetWebTablePage<T>, mut check: F) -> Result<(), String>
where
    F: FnMut(i64, &Vec<String>) -> Result<(), String>,
{
    let mut pages = table.server_key_pages.lock()
        .map_err(|_| "server table page identity registry is unavailable".to_string())?;
    for (index, keys) in pages.iter() {
        check(*index, keys)?;
    }
    pages.insert(page.page_index, page.row_keys.clone());
    Ok(())
}
pub(crate) fn jet_table_loading_store_raw<T>(table: &JetWebTable<T>, value: bool) {
    table.loading.set(value);
}
pub(crate) fn jet_table_error_store_raw<T>(table: &JetWebTable<T>, value: String) {
    table.error.set(value);
}
pub(crate) fn jet_table_page_store_raw<T: Clone + Send + Sync + 'static>(table: &JetWebTable<T>, page: Option<JetWebTablePage<T>>) {
    table.loaded_page.set(page);
}
pub(crate) fn jet_table_state_store_raw<T>(table: &JetWebTable<T>, state: JetWebTableState) {
    table.state.set(state);
}

pub(crate) fn jet_table_set_state_raw<T: Clone + Send + Sync + 'static>(table: &JetWebTable<T>, state: JetWebTableState) {
    table.set_state(state);
}
pub(crate) fn jet_table_alias_raw<T: Clone>(table: &JetWebTable<T>) -> JetWebTable<T> {
    table.clone()
}
pub(crate) fn jet_table_generation_raw<T>(table: &JetWebTable<T>, advance: bool) -> u64 {
    jet_web_table_page_generation(&table.name, advance)
}
pub(crate) fn jet_table_page_facts_raw<T>(page: &JetWebTablePage<T>) -> (i64, i64, i64, i64, i64, i64) {
    (page.total_rows, page.page_index, page.page_size, page.page_count, page.rows.len() as i64, page.row_keys.len() as i64)
}
pub(crate) fn jet_table_has_key_raw<T>(table: &JetWebTable<T>) -> bool {
    table.row_key.is_some()
}
pub(crate) fn jet_table_page_accessor_keys_raw<T>(table: &JetWebTable<T>, mut page: JetWebTablePage<T>) -> JetWebTablePage<T> {
    let key = table.row_key.as_ref().expect("checked table key accessor");
    page.row_keys = page.rows.iter().map(|row| key(row)).collect();
    page
}
pub(crate) fn jet_table_page_keys_raw<T>(page: &JetWebTablePage<T>) -> Vec<String> {
    page.row_keys.clone()
}
pub(crate) fn jet_table_page_replace_keys_raw<T>(mut page: JetWebTablePage<T>, keys: Vec<String>) -> JetWebTablePage<T> {
    page.row_keys = keys;
    page
}
pub(crate) fn jet_table_cell_raw<T>(column: &JetWebTableColumn<T>, row: &T) -> String {
    column.value(row)
}
pub(crate) fn jet_table_column_info_raw<T>(column: &JetWebTableColumn<T>) -> (String, String) {
    (column.name.clone(), column.cell_type.clone())
}
pub(crate) fn jet_table_make_page_raw<T>(rows: Vec<T>, keys: Vec<String>, total: i64, index: i64, size: i64, count: i64) -> JetWebTablePage<T> {
    JetWebTablePage { rows, row_keys: keys, total_rows: total, page_index: index, page_size: size, page_count: count }
}

pub(crate) fn jet_table_rows_raw<T: Clone + Send + Sync + 'static>(table: &JetWebTable<T>) -> Vec<T> {
    table.rows()
}
pub(crate) fn jet_table_row_key_raw<T>(table: &JetWebTable<T>, row: &T) -> String {
    (table.row_key.as_ref().expect("checked table key accessor"))(row)
}
pub(crate) fn jet_table_state_equal_raw(left: &JetWebTableState, right: &JetWebTableState) -> bool {
    left == right
}
pub(crate) fn jet_table_query_facts_raw(state: &JetWebTableState) -> (i64, i64, bool, Option<(String, bool)>, Option<(String, String)>) {
    (state.page_index, state.page_size, state.page_mode.is_server(),
        state.sort.as_ref().map(|sort| (sort.column.clone(), sort.direction.is_descending())),
        state.filter.as_ref().map(|filter| (filter.column.clone(), filter.value.clone())))
}

