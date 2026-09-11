// D-DX-SUITE1=C / #2475: headless typed web tables.
//
// The table owns only row and view state.  It emits no markup or styles.  The
// row type stays generic, while the checked compiler supplies the column
// accessor and cell type.  Runtime sorting/filtering is deliberately stable
// and deterministic so every renderer sees the same page.

use std::collections::{BTreeMap as JetWebTablePages, BTreeSet as JetWebTableSet};
use std::sync::{Arc as JetWebTableArc, Mutex as JetWebTableMutex};

const JET_WEB_TABLE_DEFAULT_PAGE_SIZE: i64 = 25;
const JET_WEB_TABLE_MAX_PAGE_SIZE: i64 = 10_000;
const JET_WEB_TABLE_MAX_KEY_BYTES: usize = 256;

/// Page requests use the same explicit-key/generation identity as #2473's
/// keyed LiveQuery registry.  The table keeps typed pages, while LiveQuery
/// owns the stale-response rule instead of this feature inventing one.
fn jet_web_table_page_generation(name: &str, advance: bool) -> u64 {
    let mut hash = 14_695_981_039_346_656_037_u64;
    for byte in name.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    let identity = format!("web.table.page.{hash:016x}");
    if advance {
        let _ = jet_app_invalidate_key(identity.clone());
    }
    jet_app_live_keyed(
        identity.clone(),
        identity,
        String::new(),
        None,
        None,
    )
    .lifecycle.generation
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetWebTableSortDirection {
    Ascending,
    Descending,
}

impl JetWebTableSortDirection {
    fn is_descending(self) -> bool {
        matches!(self, Self::Descending)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetWebTableSort {
    pub column: String,
    pub direction: JetWebTableSortDirection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetWebTableFilter {
    pub column: String,
    pub value: String,

}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetWebTablePageMode {
    Client,
    Server,
}

impl JetWebTablePageMode {
    fn is_server(self) -> bool {
        matches!(self, Self::Server)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetWebTableStatus {
    Idle,
    Loading,
    Ready,
    Error,
}

impl JetWebTableStatus {
    pub fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Loading => "loading",
            Self::Ready => "ready",
            Self::Error => "error",
        }
    }
}

/// Renderer-neutral table facts. The page and keyed rows are optional because
/// a server table has no data until its loader succeeds. DOM, TUI, and native
/// hosts consume this carrier and decide markup, colors, and layout themselves.
#[derive(Clone)]
pub struct JetWebTableProjection<T> {
    pub key: String,
    pub state: JetWebTableState,
    pub columns: Vec<(String, String)>,
    pub page: Option<JetWebTablePage<T>>,
    pub rows: Vec<JetWebTableRow<T>>,
    pub status: JetWebTableStatus,
    pub loading: bool,
    pub error: String,
}


#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetWebTableState {
    pub sort: Option<JetWebTableSort>,
    pub filter: Option<JetWebTableFilter>,
    pub page_index: i64,
    pub page_size: i64,
    /// Focus is keyed too, so virtual-window changes do not move keyboard
    /// focus to a different row when the visible slice is remeasured.
    pub selected_keys: Vec<String>,
    pub selection_anchor: Option<String>,
    pub focus_key: Option<String>,
    pub page_mode: JetWebTablePageMode,
}

impl Default for JetWebTableState {
    fn default() -> Self {
        Self {
            sort: None,
            filter: None,
            page_index: 0,
            page_size: JET_WEB_TABLE_DEFAULT_PAGE_SIZE,
            selected_keys: Vec::new(),
            selection_anchor: None,
            focus_key: None,
            page_mode: JetWebTablePageMode::Client,
        }
    }
}

/// A checked column carries its source cell type for renderer/devtools facts.
/// The accessor is the only erased part of the runtime boundary; sema rejects
/// a missing accessor before this value can be constructed from Jet.
pub struct JetWebTableColumn<T> {
    pub name: String,
    pub cell_type: String,
    accessor: JetWebTableArc<dyn Fn(&T) -> String + Send + Sync + 'static>,
}

impl<T> Clone for JetWebTableColumn<T> {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            cell_type: self.cell_type.clone(),
            accessor: self.accessor.clone(),
        }
    }
}

impl<T> JetWebTableColumn<T> {
    fn value(&self, row: &T) -> String {
        (self.accessor)(row)
    }

    pub fn cell_type(&self) -> String {
        self.cell_type.clone()
    }
}

/// One materialized page. `rows` contains only the requested page; the
/// `total_rows` and `page_count` facts remain available to an accessible
/// paginator and to the devtools panel.
#[derive(Clone, Debug, PartialEq)]
pub struct JetWebTableRow<T> {
    pub key: String,
    pub value: T,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetWebTablePage<T> {
    pub rows: Vec<T>,
    /// Keys align one-for-one with `rows`; consumers must never infer identity
    /// from the row's page position.
    pub row_keys: Vec<String>,
    pub total_rows: i64,
    pub page_index: i64,
    pub page_size: i64,
    pub page_count: i64,
}

impl<T> JetWebTablePage<T> {
    pub fn has_previous(&self) -> bool {
        self.page_index > 0
    }
    pub fn has_next(&self) -> bool {
        self.page_index.saturating_add(1) < self.page_count
    }

    pub fn first_page(&self) -> i64 {
        0
    }

    pub fn last_page(&self) -> i64 {
        self.page_count.saturating_sub(1)
    }

    pub fn next_page(&self) -> Option<i64> {
        self.has_next().then_some(self.page_index + 1)
    }

    pub fn previous_page(&self) -> Option<i64> {
        self.has_previous().then_some(self.page_index - 1)
    }

    pub fn keyed_rows(&self) -> Vec<JetWebTableRow<T>>
    where
        T: Clone,
    {
        self.rows
            .iter()
            .cloned()
            .zip(self.row_keys.iter().cloned())
            .map(|(value, key)| JetWebTableRow { key, value })
            .collect()
    }
}

type JetWebTablePageLoader<T> =
    JetWebTableArc<dyn Fn(JetWebTableState) -> Result<JetWebTablePage<T>, String> + Send + Sync>;
type JetWebTableRowKey<T> =
    JetWebTableArc<dyn Fn(&T) -> String + Send + Sync + 'static>;

#[derive(Clone)]
pub struct JetWebTable<T> {
    pub name: String,
    rows: jet_std::JetSignal<Vec<T>>,
    state: jet_std::JetSignal<JetWebTableState>,
    columns: JetWebTableArc<Vec<JetWebTableColumn<T>>>,
    row_key: Option<JetWebTableRowKey<T>>,
    server_page: Option<JetWebTablePageLoader<T>>,
    loading: jet_std::JetSignal<bool>,
    error: jet_std::JetSignal<String>,
    /// The last successful page is the only data snapshot exposed to a
    /// renderer for a server-backed table. It is invalidated whenever the
    /// page-driving state changes.
    loaded_page: jet_std::JetSignal<Option<JetWebTablePage<T>>>,
    /// Accepted row keys by page for the current sort/filter boundary. A
    /// duplicate key on a different page is rejected instead of silently
    /// rendering one row twice.
    server_key_pages: JetWebTableArc<JetWebTableMutex<JetWebTablePages<i64, Vec<String>>>>,
}

impl<T: Clone + Send + Sync + 'static> JetWebTable<T> {
    pub fn new(name: String, rows: Vec<T>) -> Self {
        Self::new_with_key_arc(name, rows, None)
    }

    pub fn new_with_key<F>(name: String, rows: Vec<T>, key: F) -> Self
    where
        F: Fn(&T) -> String + Send + Sync + 'static,
    {
        Self::new_with_key_arc(name, rows, Some(JetWebTableArc::new(key)))
    }

    fn new_with_key_arc(
        name: String,
        rows: Vec<T>,
        row_key: Option<JetWebTableRowKey<T>>,
    ) -> Self {
        Self {
            name,
            rows: jet_std::JetSignal::new(rows),
            state: jet_std::JetSignal::new(JetWebTableState::default()),
            columns: JetWebTableArc::new(Vec::new()),
            row_key,
            server_page: None,
            loading: jet_std::JetSignal::new(false),
            error: jet_std::JetSignal::new(String::new()),
            loaded_page: jet_std::JetSignal::new(None),
            server_key_pages: JetWebTableArc::new(JetWebTableMutex::new(
                JetWebTablePages::new(),
            )),
        }
    }

    pub fn rows(&self) -> Vec<T> {
        self.rows.get()
    }

    pub fn rows_signal(&self) -> jet_std::JetSignal<Vec<T>> {
        self.rows.clone()
    }

    fn clear_server_key_pages(&self) {
        if let Ok(mut pages) = self.server_key_pages.lock() {
            pages.clear();
        }
    }

    fn accept_server_page_keys(&self, page: &JetWebTablePage<T>) -> Result<(), String> {
        let Ok(mut pages) = self.server_key_pages.lock() else {
            return Err("server table page identity registry is unavailable".to_string());
        };
        for (other_page, keys) in pages.iter() {
            if *other_page != page.page_index
                && page
                    .row_keys
                    .iter()
                    .any(|key| keys.iter().any(|other| other == key))
            {
                return Err("server table response duplicates a row key across pages".to_string());
            }
        }
        pages.insert(page.page_index, page.row_keys.clone());
        Ok(())
    }

    pub fn set_rows(&self, rows: Vec<T>) {
        let state = jet_web_table_reconcile_state(
            self.state(),
            &rows,
            self.row_key.as_ref(),
        );
        self.rows.set(rows);
        self.state.set(state);
        self.loaded_page.set(None);
        self.error.set(String::new());
        self.loading.set(false);
        if self.server_page.is_some() {
            self.clear_server_key_pages();
            let _ = jet_web_table_page_generation(&self.name, true);
        }
    }

    pub fn with_key<F>(mut self, key: F) -> Self
    where
        F: Fn(&T) -> String + Send + Sync + 'static,
    {
        self.row_key = Some(JetWebTableArc::new(key));
        let state = jet_web_table_reconcile_state(
            self.state(),
            &self.rows(),
            self.row_key.as_ref(),
        );
        self.state.set(state);
        self.loaded_page.set(None);
        self.loading.set(false);
        if self.server_page.is_some() {
            self.clear_server_key_pages();
            let _ = jet_web_table_page_generation(&self.name, true);
        }
        self
    }

    pub fn has_explicit_key(&self) -> bool {
        self.row_key.is_some()
    }

    pub fn state(&self) -> JetWebTableState {
        self.state.get()
    }

    pub fn state_signal(&self) -> jet_std::JetSignal<JetWebTableState> {
        self.state.clone()
    }

    pub fn set_state(&self, state: JetWebTableState) {
        let previous = self.state();
        let normalized = jet_web_table_normalize_state(state);
        if jet_web_table_page_state_changed(&previous, &normalized) {
            self.loaded_page.set(None);
            if self.server_page.is_some() {
                if jet_web_table_query_boundary_changed(&previous, &normalized) {
                    self.clear_server_key_pages();
                }
                self.loading.set(false);
                let _ = jet_web_table_page_generation(&self.name, true);
            }
        }
        self.state.set(normalized);
    }

    /// Add a checked column without mutating existing table handles.  The
    /// table's signals remain shared, so adding a column is configuration only.
    pub fn with_column(mut self, column: JetWebTableColumn<T>) -> Self {
        let mut columns = (*self.columns).clone();
        columns.push(column);
        self.columns = JetWebTableArc::new(columns);
        self
    }
    pub fn with_server_page<F>(mut self, loader: F) -> Self
    where
        F: Fn(JetWebTableState) -> Result<JetWebTablePage<T>, String>
            + Send
            + Sync
            + 'static,
    {
        let was_server = self.state().page_mode.is_server();
        self.server_page = Some(JetWebTableArc::new(loader));
        let mut state = self.state();
        state.page_mode = JetWebTablePageMode::Server;
        if was_server {
            self.clear_server_key_pages();
            self.loading.set(false);
            let _ = jet_web_table_page_generation(&self.name, true);
        }
        self.set_state(state);
        self.loaded_page.set(None);
        self.error.set(String::new());
        self
    }

    pub fn loading(&self) -> bool {
        self.loading.get()
    }

    pub fn loading_signal(&self) -> jet_std::JetSignal<bool> {
        self.loading.clone()
    }

    pub fn loaded_page(&self) -> Option<JetWebTablePage<T>> {
        self.loaded_page.get()
    }

    pub fn status(&self) -> JetWebTableStatus {
        if self.loading() {
            JetWebTableStatus::Loading
        } else if !self.error().is_empty() {
            JetWebTableStatus::Error
        } else if self.loaded_page().is_some() {
            JetWebTableStatus::Ready
        } else {
            JetWebTableStatus::Idle
        }
    }

    /// Return one renderer-neutral snapshot. Local tables materialize their
    /// current page on first projection; server tables never invoke a loader
    /// implicitly and therefore remain idle until `page()` succeeds.
    pub fn projection(&self) -> JetWebTableProjection<T> {
        let state = self.state();
        let page = match self.loaded_page() {
            Some(page) => Some(page),
            None if !state.page_mode.is_server() => self.page().ok(),
            None => None,
        };
        let rows = page
            .as_ref()
            .map(JetWebTablePage::keyed_rows)
            .unwrap_or_default();
        JetWebTableProjection {
            key: self.name.clone(),
            state: self.state(),
            columns: self.columns(),
            page,
            rows,
            status: self.status(),
            loading: self.loading(),
            error: self.error(),
        }
    }

    pub fn error(&self) -> String {
        self.error.get()
    }

    pub fn error_signal(&self) -> jet_std::JetSignal<String> {
        self.error.clone()
    }

    pub fn columns(&self) -> Vec<(String, String)> {
        self.columns
            .iter()
            .map(|column| (column.name.clone(), column.cell_type.clone()))
            .collect()
    }

    pub fn sort_by(&self, column: String, direction: JetWebTableSortDirection) -> Self {
        let mut state = self.state();
        state.sort = Some(JetWebTableSort { column, direction });
        state.page_index = 0;
        self.set_state(state);
        self.clone()
    }

    pub fn clear_sort(&self) -> Self {
        let mut state = self.state();
        state.sort = None;
        state.page_index = 0;
        self.set_state(state);
        self.clone()
    }

    pub fn filter_by(&self, column: String, value: String) -> Self {
        let mut state = self.state();
        state.filter = Some(JetWebTableFilter { column, value });
        state.page_index = 0;
        self.set_state(state);
        self.clone()
    }

    pub fn clear_filter(&self) -> Self {
        let mut state = self.state();
        state.filter = None;
        state.page_index = 0;
        self.set_state(state);
        self.clone()
    }

    pub fn paginate(&self, page_index: i64, page_size: i64) -> Self {
        let mut state = self.state();
        state.page_index = page_index.max(0);
        state.page_size = page_size.clamp(1, JET_WEB_TABLE_MAX_PAGE_SIZE);
        self.set_state(state);
        self.clone()
    }

    pub fn page(&self) -> Result<JetWebTablePage<T>, String> {
        let state = self.state();
        if state.page_mode.is_server() && self.server_page.is_none() {
            let error = "server table state requires a page loader".to_string();
            self.loading.set(false);
            self.loaded_page.set(None);
            self.error.set(error.clone());
            return Err(error);
        }
        if let Some(loader) = self.server_page.as_ref() {
            // The keyed LiveQuery registry owns request generations. Advancing
            // before every loader call means a later page request supersedes
            // this one even when both requests have identical table state.
            let request_generation = jet_web_table_page_generation(&self.name, true);
            self.loading.set(true);
            self.error.set(String::new());
            let loaded = loader(state.clone());
            let current_generation = jet_web_table_page_generation(&self.name, false);
            if current_generation != request_generation || self.state() != state {
                if current_generation == request_generation {
                    self.loading.set(false);
                }
                return Err("server table page response is stale".to_string());
            }
            let result = loaded
                .and_then(|page| {
                    if page.page_index != state.page_index || page.page_size != state.page_size {
                        return Err(
                            "server table page response does not match the requested boundary"
                                .to_string(),
                        );
                    }
                    jet_web_table_validate_server_page(page, self.row_key.as_ref())
                });
            match result {
                Ok(page) => {
                    if jet_web_table_page_generation(&self.name, false) != request_generation
                        || self.state() != state
                    {
                        return Err("server table page response is stale".to_string());
                    }
                    if let Err(error) = self.accept_server_page_keys(&page) {
                        self.loading.set(false);
                        self.loaded_page.set(None);
                        self.error.set(error.clone());
                        return Err(error);
                    }
                    self.loading.set(false);
                    let mut canonical = state;
                    canonical.page_index = page.page_index;
                    canonical.page_size = page.page_size;
                    self.state.set(canonical);
                    self.loaded_page.set(Some(page.clone()));
                    self.error.set(String::new());
                    Ok(page)
                }
                Err(error) => {
                    if jet_web_table_page_generation(&self.name, false) != request_generation
                        || self.state() != state
                    {
                        return Err("server table page response is stale".to_string());
                    }
                    self.loading.set(false);
                    self.loaded_page.set(None);
                    self.error.set(error.clone());
                    Err(error)
                }
            }
        } else {
            let result = jet_web_table_materialize(
                &self.rows(),
                &state,
                &self.columns,
                self.row_key.as_ref(),
            );
            match result {
                Ok(page) => {
                    let mut canonical = state;
                    canonical.page_index = page.page_index;
                    canonical.page_size = page.page_size;
                    self.state.set(canonical);
                    self.loaded_page.set(Some(page.clone()));
                    self.error.set(String::new());
                    Ok(page)
                }
                Err(error) => {
                    self.loaded_page.set(None);
                    self.error.set(error.clone());
                    Err(error)
                }
            }
        }
    }
    pub fn derived_page(&self) -> jet_std::JetDerived<Result<JetWebTablePage<T>, String>> {
        let rows = self.rows.clone();
        let state = self.state.clone();
        let columns = self.columns.clone();
        let row_key = self.row_key.clone();
        let server = self.server_page.is_some();
        jet_std::JetDerived::new(move || {
            if server || state.get().page_mode.is_server() {
                Err("server table pages require explicit page()".to_string())
            } else {
                jet_web_table_materialize(&rows.get(), &state.get(), &columns, row_key.as_ref())
            }
        })
    }

    pub fn facts_json(&self) -> String {
        let state = self.state();
        let columns = self.columns();
        let column_json = columns
            .iter()
            .map(|(name, cell_type)| {
                format!(
                    "{{\"name\":\"{}\",\"type\":\"{}\"}}",
                    jet_web_table_json_escape(name),
                    jet_web_table_json_escape(cell_type)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let page = self.loaded_page();
        let total_rows = page.as_ref().map(|page| page.total_rows).unwrap_or(0);
        let page_count = page.as_ref().map(|page| page.page_count).unwrap_or(0);
        let status = self.status();
        let error = self.error();
        format!(
            "{{\"name\":\"{}\",\"identity\":\"{}\",\"key\":\"{}\",\"columns\":[{}],\"page_index\":{},\"page_size\":{},\"page_mode\":\"{}\",\"selected_keys\":[{}],\"selection_anchor\":{},\"focus_key\":{},\"has_explicit_key\":{},\"sort\":{},\"filter\":{},\"loaded_page\":{},\"total_rows\":{},\"page_count\":{},\"status\":\"{}\",\"loading\":{},\"error\":\"{}\"}}",
            jet_web_table_json_escape(&self.name),
            jet_web_table_json_escape(&self.name),
            jet_web_table_json_escape(&self.name),
            column_json,
            state.page_index.max(0),
            state.page_size.clamp(1, JET_WEB_TABLE_MAX_PAGE_SIZE),
            if state.page_mode.is_server() { "server" } else { "client" },
            jet_web_table_string_list_json(&state.selected_keys),
            state
                .selection_anchor
                .as_ref()
                .map(|key| format!("\"{}\"", jet_web_table_json_escape(key)))
                .unwrap_or_else(|| "null".to_string()),
            state
                .focus_key
                .as_ref()
                .map(|key| format!("\"{}\"", jet_web_table_json_escape(key)))
                .unwrap_or_else(|| "null".to_string()),
            self.has_explicit_key(),
            state
                .sort
                .as_ref()
                .map(|sort| {
                    format!(
                        "{{\"column\":\"{}\",\"direction\":\"{}\"}}",
                        jet_web_table_json_escape(&sort.column),
                        if sort.direction.is_descending() { "desc" } else { "asc" }
                    )
                })
                .unwrap_or_else(|| "null".to_string()),
            state
                .filter
                .as_ref()
                .map(|filter| {
                    format!(
                        "{{\"column\":\"{}\",\"value\":\"{}\"}}",
                        jet_web_table_json_escape(&filter.column),
                        jet_web_table_json_escape(&filter.value)
                    )
                })
                .unwrap_or_else(|| "null".to_string()),
            page.is_some(),
            total_rows,
            page_count,
            status.name(),
            self.loading(),
            jet_web_table_json_escape(&error),
        )
    }
}

impl<T: Clone + Send + Sync + 'static> JetWebTable<T> {
    pub fn row_keys(&self) -> Result<Vec<String>, String> {
        if self.server_page.is_some() {
            if let Some(page) = self.loaded_page() {
                return Ok(page.row_keys);
            }
        }
        Ok(jet_web_table_rows_with_keys(
            &self.rows(),
            self.row_key.as_ref(),
        )?
        .into_iter()
        .map(|row| row.key)
        .collect())
    }

    pub fn rows_with_keys(&self) -> Result<Vec<JetWebTableRow<T>>, String> {
        if self.server_page.is_some() {
            if let Some(page) = self.loaded_page() {
                return Ok(page.keyed_rows());
            }
        }
        jet_web_table_rows_with_keys(&self.rows(), self.row_key.as_ref())
    }

    pub fn selected_keys(&self) -> Vec<String> {
        self.state().selected_keys
    }
    pub fn focused_key(&self) -> Option<String> {
        self.state().focus_key
    }

    pub fn focus(&self, key: String) -> Result<Self, String> {
        if !jet_web_table_key_is_valid(&key) {
            return Err("table focus key must be non-empty, bounded, and printable".to_string());
        }
        let mut state = self.state();
        state.focus_key = Some(key);
        self.set_state(state);
        Ok(self.clone())
    }

    pub fn clear_focus(&self) -> Self {
        let mut state = self.state();
        state.focus_key = None;
        self.set_state(state);
        self.clone()
    }


    pub fn set_selected(&self, key: String, selected: bool) -> Result<Self, String> {
        if !jet_web_table_key_is_valid(&key) {
            return Err("table selection key must be non-empty, bounded, and printable".to_string());
        }
        let mut state = self.state();
        if selected {
            state.selected_keys.push(key.clone());
        } else {
            state.selected_keys.retain(|existing| existing != &key);
        }
        if selected {
            state.selection_anchor = Some(key.clone());
            state.focus_key = Some(key);
        } else if state.selection_anchor.as_deref() == Some(key.as_str()) {
            state.selection_anchor = state.selected_keys.first().cloned();
        }
        self.set_state(state);
        Ok(self.clone())
    }

    pub fn toggle_selection(&self, key: String) -> Result<Self, String> {
        let selected = self.state().selected_keys.iter().any(|value| value == &key);
        self.set_selected(key, !selected)
    }

    pub fn clear_selection(&self) -> Self {
        let mut state = self.state();
        state.selected_keys.clear();
        state.selection_anchor = None;
        self.set_state(state);
        self.clone()
    }

    pub fn selected_rows(&self) -> Result<Vec<JetWebTableRow<T>>, String> {
        let selected = self
            .state()
            .selected_keys
            .into_iter()
            .collect::<JetWebTableSet<_>>();
        let rows = if self.server_page.is_some() {
            match self.loaded_page() {
                Some(page) => page.keyed_rows(),
                None => self.rows_with_keys()?,
            }
        } else {
            self.rows_with_keys()?
        };
        Ok(rows
            .into_iter()
            .filter(|row| selected.contains(&row.key))
            .collect())
    }

    pub fn replace_rows(&self, rows: Vec<T>) -> Result<(), String> {
        jet_web_table_rows_with_keys(&rows, self.row_key.as_ref())?;
        self.set_rows(rows);
        Ok(())
    }

    pub fn insert_row(&self, row: T) -> Result<String, String> {
        if self.row_key.is_none() {
            return Err("table insertion requires an explicit row key".to_string());
        }
        let key = self
            .row_key
            .as_ref()
            .map(|key| key(&row))
            .unwrap_or_default();
        if !jet_web_table_key_is_valid(&key) {
            return Err("table row key must be non-empty, bounded, and printable".to_string());
        }
        let mut rows = self.rows();
        let existing = jet_web_table_rows_with_keys(&rows, self.row_key.as_ref())?;
        if existing.iter().any(|existing| existing.key == key) {
            return Err(format!("table row key `{key}` is duplicated"));
        }
        rows.push(row);
        self.set_rows(rows);
        Ok(key)
    }

    pub fn replace_row(&self, key: &str, row: T) -> Result<(), String> {
        if !jet_web_table_key_is_valid(key) {
            return Err("table row key must be non-empty, bounded, and printable".to_string());
        }
        let mut rows = self.rows();
        let keyed = jet_web_table_rows_with_keys(&rows, self.row_key.as_ref())?;
        let index = keyed
            .iter()
            .position(|existing| existing.key == key)
            .ok_or_else(|| format!("table row key `{key}` was not found"))?;
        let replacement_key = self
            .row_key
            .as_ref()
            .map(|row_key| row_key(&row))
            .unwrap_or_else(|| format!("index:{index}"));
        if replacement_key != key {
            return Err("table row replacement cannot change the row key".to_string());
        }
        rows[index] = row;
        jet_web_table_rows_with_keys(&rows, self.row_key.as_ref())?;
        self.set_rows(rows);
        Ok(())
    }

    pub fn update_row<F>(&self, key: &str, update: F) -> Result<(), String>
    where
        F: FnOnce(&mut T),
    {
        if !jet_web_table_key_is_valid(key) {
            return Err("table row key must be non-empty, bounded, and printable".to_string());
        }
        let mut rows = self.rows();
        let keyed = jet_web_table_rows_with_keys(&rows, self.row_key.as_ref())?;
        let index = keyed
            .iter()
            .position(|row| row.key == key)
            .ok_or_else(|| format!("table row key `{key}` was not found"))?;
        update(&mut rows[index]);
        let updated_key = self
            .row_key
            .as_ref()
            .map(|row_key| row_key(&rows[index]))
            .unwrap_or_else(|| format!("index:{index}"));
        if updated_key != key {
            return Err("table row updates cannot change the row key".to_string());
        }
        jet_web_table_rows_with_keys(&rows, self.row_key.as_ref())?;
        self.set_rows(rows);
        Ok(())
    }

    pub fn remove_row(&self, key: &str) -> Result<T, String> {
        let mut rows = self.rows();
        let keyed = jet_web_table_rows_with_keys(&rows, self.row_key.as_ref())?;
        let index = keyed
            .iter()
            .position(|row| row.key == key)
            .ok_or_else(|| format!("table row key `{key}` was not found"))?;
        let removed = rows.remove(index);
        self.set_rows(rows);
        Ok(removed)
    }

    pub fn first_page(&self) -> Self {
        self.paginate(0, self.state().page_size)
    }
    pub fn next_page(&self) -> Self {
        let page = match self.loaded_page() {
            Some(page) => page,
            None => {
                let Ok(page) = self.page() else {
                    return self.clone();
                };
                page
            }
        };
        let Some(next_index) = page.next_page() else {
            return self.clone();
        };
        self.paginate(next_index, page.page_size)
    }

    pub fn last_page(&self) -> Result<Self, String> {
        let page = if let Some(page) = self.loaded_page() {
            page
        } else {
            self.page()?
        };
        Ok(self.paginate(page.last_page(), page.page_size))
    }

    pub fn visible_rows(
        &self,
        plan: &JetWebVirtualPlan,
    ) -> Result<Vec<JetWebTableRow<T>>, String> {
        if self.server_page.is_some() {
            let page = self
                .loaded_page()
                .ok_or_else(|| "server table visible rows require a loaded page".to_string())?;
            let rows = page.keyed_rows();
            let loaded_count = i64::try_from(rows.len()).unwrap_or(i64::MAX);
            let global_indices =
                plan.total_count == page.total_rows && plan.total_count > loaded_count;
            let page_start = page
                .page_index
                .saturating_mul(page.page_size)
                .max(0);
            // A plan over the complete server result uses global indices. A
            // plan over the loaded page uses local indices; both remain
            // bounded by the plan's canonical visible range.
            return Ok(plan
                .indices()
                .into_iter()
                .filter_map(|index| {
                    let local = if global_indices {
                        index.checked_sub(page_start)?
                    } else {
                        index
                    };
                    usize::try_from(local).ok().and_then(|local| rows.get(local).cloned())
                })
                .collect());
        }
        let state = self.state();
        let rows = jet_web_table_rows_with_keys(&self.rows(), self.row_key.as_ref())?;
        let mut visible = rows;
        if let Some(filter) = &state.filter {
            let column = self
                .columns
                .iter()
                .find(|column| column.name == filter.column)
                .ok_or_else(|| format!("table filter column `{}` has no accessor", filter.column))?;
            visible.retain(|row| column.value(&row.value).contains(filter.value.as_str()));
        }
        if let Some(sort) = &state.sort {
            let column = self
                .columns
                .iter()
                .find(|column| column.name == sort.column)
                .ok_or_else(|| format!("table sort column `{}` has no accessor", sort.column))?;
            let descending = sort.direction.is_descending();
            visible.sort_by(|left, right| {
                let ordering = column.value(&left.value).cmp(&column.value(&right.value));
                if descending {
                    ordering.reverse()
                } else {
                    ordering
                }
            });
        }
        Ok(plan
            .indices()
            .into_iter()
            .filter_map(|index| usize::try_from(index).ok())
            .filter_map(|index| visible.get(index).cloned())
            .collect())
    }

    /// Renderers provide the node type. The table only supplies keyed rows, so
    /// DOM, TUI, and native adapters consume one semantic result without
    /// introducing a markup or style policy here.
    pub fn render_page<R, F>(&self, render: F) -> Result<Vec<R>, String>
    where
        F: Fn(&T, &str) -> R,
    {
        Ok(self
            .page()?
            .keyed_rows()
            .iter()
            .map(|row| render(&row.value, &row.key))
            .collect())
    }

    pub fn render_virtual<R, F>(
        &self,
        plan: &JetWebVirtualPlan,
        render: F,
    ) -> Result<Vec<R>, String>
    where
        F: Fn(&T, &str) -> R,
    {
        Ok(self
            .visible_rows(plan)?
            .iter()
            .map(|row| render(&row.value, &row.key))
            .collect())
    }
}

/// Core-call adapters for table methods. They keep the table value as the
/// semantic receiver while each host decides how to marshal the result.
pub fn jet_web_table_new_keyed<T, F>(
    name: String,
    rows: Vec<T>,
    key: F,
) -> JetWebTable<T>
where
    T: Clone + Send + Sync + 'static,
    F: Fn(&T) -> String + Send + Sync + 'static,
{
    jet_web_table_with_key(name, rows, key)
}

pub fn jet_web_table_with_column<T: Clone + Send + Sync + 'static>(
    table: &JetWebTable<T>,
    column: JetWebTableColumn<T>,
) -> JetWebTable<T> {
    table.clone().with_column(column)
}

pub fn jet_web_table_with_server_page<T, F>(
    table: &JetWebTable<T>,
    loader: F,
) -> JetWebTable<T>
where
    T: Clone + Send + Sync + 'static,
    F: Fn(JetWebTableState) -> Result<JetWebTablePage<T>, String>
        + Send
        + Sync
        + 'static,
{
    table.clone().with_server_page(loader)
}

pub fn jet_web_table_state<T: Clone + Send + Sync + 'static>(
    table: &JetWebTable<T>,
) -> JetWebTableState {
    table.state()
}

pub fn jet_web_table_facts<T: Clone + Send + Sync + 'static>(
    table: &JetWebTable<T>,
) -> String {
    table.facts_json()
}

/// Host-only projection adapter for DOM, TUI, and native consumers. It
/// returns keyed page rows and state facts without choosing a renderer.
pub fn jet_web_table_projection<T: Clone + Send + Sync + 'static>(
    table: &JetWebTable<T>,
) -> JetWebTableProjection<T> {
    table.projection()
}

pub fn jet_web_table_keys<T: Clone + Send + Sync + 'static>(
    table: &JetWebTable<T>,
) -> Result<Vec<String>, String> {
    table.row_keys()
}

pub fn jet_web_table_page_state<T: Clone + Send + Sync + 'static>(
    table: &JetWebTable<T>,
) -> Result<JetWebTablePage<T>, String> {
    table.page()
}

pub fn jet_web_table_sort_by<T: Clone + Send + Sync + 'static>(
    table: &JetWebTable<T>,
    column: String,
    direction: JetWebTableSortDirection,
) -> JetWebTable<T> {
    table.sort_by(column, direction)
}

pub fn jet_web_table_filter_by<T: Clone + Send + Sync + 'static>(
    table: &JetWebTable<T>,
    column: String,
    value: String,
) -> JetWebTable<T> {
    table.filter_by(column, value)
}

pub fn jet_web_table_paginate<T: Clone + Send + Sync + 'static>(
    table: &JetWebTable<T>,
    page_index: i64,
    page_size: i64,
) -> JetWebTable<T> {
    table.paginate(page_index, page_size)
}

pub fn jet_web_table_set_rows<T: Clone + Send + Sync + 'static>(
    table: &JetWebTable<T>,
    rows: Vec<T>,
) -> Result<(), String> {
    table.replace_rows(rows)
}

pub fn jet_web_table_set_selected<T: Clone + Send + Sync + 'static>(
    table: &JetWebTable<T>,
    key: String,
    selected: bool,
) -> Result<JetWebTable<T>, String> {
    table.set_selected(key, selected)
}

pub fn jet_web_table_toggle_selection<T: Clone + Send + Sync + 'static>(
    table: &JetWebTable<T>,
    key: String,
) -> Result<JetWebTable<T>, String> {
    table.toggle_selection(key)
}
pub fn jet_web_table_focus<T: Clone + Send + Sync + 'static>(
    table: &JetWebTable<T>,
    key: String,
) -> Result<JetWebTable<T>, String> {
    table.focus(key)
}

pub fn jet_web_table_clear_focus<T: Clone + Send + Sync + 'static>(
    table: &JetWebTable<T>,
) -> JetWebTable<T> {
    table.clear_focus()
}

pub fn jet_web_table_focused_key<T: Clone + Send + Sync + 'static>(
    table: &JetWebTable<T>,
) -> Option<String> {
    table.focused_key()
}


pub fn jet_web_table_clear_selection<T: Clone + Send + Sync + 'static>(
    table: &JetWebTable<T>,
) -> JetWebTable<T> {
    table.clear_selection()
}

pub fn jet_web_table_selected_keys<T: Clone + Send + Sync + 'static>(
    table: &JetWebTable<T>,
) -> Vec<String> {
    table.selected_keys()
}

pub fn jet_web_table_selected_rows<T: Clone + Send + Sync + 'static>(
    table: &JetWebTable<T>,
) -> Result<Vec<JetWebTableRow<T>>, String> {
    table.selected_rows()
}

pub fn jet_web_table_insert_row<T: Clone + Send + Sync + 'static>(
    table: &JetWebTable<T>,
    row: T,
) -> Result<String, String> {
    table.insert_row(row)
}

pub fn jet_web_table_replace_row<T: Clone + Send + Sync + 'static>(
    table: &JetWebTable<T>,
    key: String,
    row: T,
) -> Result<(), String> {
    table.replace_row(&key, row)
}

pub fn jet_web_table_update_row<T: Clone + Send + Sync + 'static, F>(
    table: &JetWebTable<T>,
    key: String,
    update: F,
) -> Result<(), String>
where
    F: FnOnce(&mut T),
{
    table.update_row(&key, update)
}

pub fn jet_web_table_remove_row<T: Clone + Send + Sync + 'static>(
    table: &JetWebTable<T>,
    key: String,
) -> Result<T, String> {
    table.remove_row(&key)
}

pub fn jet_web_table_first_page<T: Clone + Send + Sync + 'static>(
    table: &JetWebTable<T>,
) -> JetWebTable<T> {
    table.first_page()
}

pub fn jet_web_table_next_page<T: Clone + Send + Sync + 'static>(
    table: &JetWebTable<T>,
) -> JetWebTable<T> {
    table.next_page()
}

pub fn jet_web_table_last_page<T: Clone + Send + Sync + 'static>(
    table: &JetWebTable<T>,
) -> Result<JetWebTable<T>, String> {
    table.last_page()
}

pub fn jet_web_table_visible_rows<T: Clone + Send + Sync + 'static>(
    table: &JetWebTable<T>,
    plan: &JetWebVirtualPlan,
) -> Result<Vec<JetWebTableRow<T>>, String> {
    table.visible_rows(plan)
}

pub fn jet_web_virtual_plan_viewport(plan: JetWebVirtualPlan) -> JetWebVirtualPlanViewport {
    JetWebVirtualPlanViewport::new(plan)
}

/// Core-call adapters for the stateful virtual plan viewport.
pub fn jet_web_virtual_plan_measure(
    plan: &JetWebVirtualPlan,
    index: i64,
    size: i64,
) -> JetWebVirtualPlan {
    plan.with_measurement(index, size)
}

pub fn jet_web_virtual_plan_viewport_state(
    viewport: &JetWebVirtualPlanViewport,
) -> JetWebVirtualPlan {
    viewport.plan()
}

pub fn jet_web_virtual_plan_scroll_to(
    viewport: &JetWebVirtualPlanViewport,
    scroll_offset: i64,
) {
    viewport.scroll_to(scroll_offset);
}

pub fn jet_web_virtual_plan_resize(
    viewport: &JetWebVirtualPlanViewport,
    viewport_width: i64,
    viewport_height: i64,
) {
    viewport.resize(viewport_width, viewport_height);
}

pub fn jet_web_virtual_plan_viewport_measure(
    viewport: &JetWebVirtualPlanViewport,
    index: i64,
    size: i64,
) {
    viewport.measure(index, size);
}

pub fn jet_web_virtual_plan_facts(plan: &JetWebVirtualPlan) -> String {
    plan.facts_json()
}

/// Construct a table over a checked row list.  `name` is a panel identity, not
/// an HTML element; renderers decide how to present the headless result.
pub fn jet_web_table<T: Clone + Send + Sync + 'static>(
    name: String,
    rows: Vec<T>,
) -> JetWebTable<T> {
    JetWebTable::new(name, rows)
}

/// Construct a table with the explicit stable row identity required for
/// reorder, insertion, and selection reconciliation.
pub fn jet_web_table_with_key<T, F>(
    name: String,
    rows: Vec<T>,
    key: F,
) -> JetWebTable<T>
where
    T: Clone + Send + Sync + 'static,
    F: Fn(&T) -> String + Send + Sync + 'static,
{
    JetWebTable::new_with_key(name, rows, key)
}

pub fn jet_web_table_column<T, F>(
    name: String,
    cell_type: String,
    accessor: F,
) -> JetWebTableColumn<T>
where
    F: Fn(&T) -> String + Send + Sync + 'static,
{
    JetWebTableColumn {
        name,
        cell_type,
        accessor: JetWebTableArc::new(accessor),
    }
}

pub fn jet_web_table_sort<T: Clone, F>(
    rows: &Vec<T>,
    descending: bool,
    key: F,
) -> Vec<T>
where
    F: Fn(&T) -> String,
{
    let mut sorted = rows.clone();
    sorted.sort_by(|left, right| {
        let ordering = key(left).cmp(&key(right));
        if descending {
            ordering.reverse()
        } else {
            ordering
        }
    });
    sorted
}

pub fn jet_web_table_filter<T: Clone, F>(rows: &Vec<T>, predicate: F) -> Vec<T>
where
    F: Fn(&T) -> bool,
{
    rows.iter().filter(|row| predicate(row)).cloned().collect()
}

pub fn jet_web_table_page<T: Clone>(
    rows: &Vec<T>,
    page_index: i64,
    page_size: i64,
) -> JetWebTablePage<T> {
    let page_size = page_size.clamp(1, JET_WEB_TABLE_MAX_PAGE_SIZE);
    let total_rows = i64::try_from(rows.len()).unwrap_or(i64::MAX);
    let page_count = if total_rows == 0 {
        0
    } else {
        total_rows
            .saturating_add(page_size.saturating_sub(1))
            .checked_div(page_size)
            .unwrap_or(0)
    };
    let page_index = if page_count == 0 {
        0
    } else {
        page_index.max(0).min(page_count - 1)
    };
    let start = page_index
        .saturating_mul(page_size)
        .min(total_rows)
        .try_into()
        .unwrap_or(usize::MAX)
        .min(rows.len());
    let end = start
        .saturating_add(page_size.try_into().unwrap_or(usize::MAX))
        .min(rows.len());
    JetWebTablePage {
        rows: rows[start..end].to_vec(),
        row_keys: (start..end).map(|index| format!("index:{index}")).collect(),
        total_rows,
        page_index,
        page_size,
        page_count,
    }
}
fn jet_web_table_validate_server_page<T>(
    mut page: JetWebTablePage<T>,
    row_key: Option<&JetWebTableRowKey<T>>,
) -> Result<JetWebTablePage<T>, String> {
    if page.total_rows < 0 {
        return Err("server table page has negative total_rows".to_string());
    }
    if page.page_size < 1 || page.page_size > JET_WEB_TABLE_MAX_PAGE_SIZE {
        return Err(format!(
            "server table page_size must be between 1 and {}",
            JET_WEB_TABLE_MAX_PAGE_SIZE
        ));
    }
    if page.page_index < 0 {
        return Err("server table page has negative page_index".to_string());
    }
    if page.rows.len() > page.page_size as usize {
        return Err("server table returned more rows than page_size".to_string());
    }
    let expected_count = if page.total_rows == 0 {
        0
    } else {
        page.total_rows
            .saturating_add(page.page_size - 1)
            .checked_div(page.page_size)
            .unwrap_or(0)
    };
    if page.page_count != expected_count {
        return Err("server table page_count does not match total_rows".to_string());
    }
    if expected_count == 0 {
        if page.page_index != 0 || !page.rows.is_empty() || !page.row_keys.is_empty() {
            return Err("empty server table page has invalid page_index, rows, or keys".to_string());
        }
    } else if page.page_index >= expected_count {
        return Err("server table page_index is out of range".to_string());
    }
    if page.total_rows > 0 && page.rows.is_empty() {
        return Err("non-empty server table result returned an empty page".to_string());
    }
    if expected_count > 0
        && page.page_index.saturating_add(1) < expected_count
        && page.rows.len() != page.page_size as usize
    {
        return Err("non-final server table pages must be full".to_string());
    }
    if let Some(key) = row_key {
        // An explicit table key is canonical. Ignore positional or stale
        // loader keys and derive identity from the checked row accessor.
        page.row_keys = page.rows.iter().map(|row| key(row)).collect();
    } else if page.row_keys.is_empty() && !page.rows.is_empty() {
        let page_start = page
            .page_index
            .saturating_mul(page.page_size)
            .max(0);
        page.row_keys = page
            .rows
            .iter()
            .enumerate()
            .map(|(offset, _)| format!("index:{}", page_start.saturating_add(offset as i64)))
            .collect();
    }
    if page.row_keys.len() != page.rows.len() {
        return Err("server table row_keys must align with rows".to_string());
    }
    jet_web_table_validate_keys(&page.row_keys)?;
    Ok(page)
}

fn jet_web_table_materialize<T: Clone>(
    rows: &Vec<T>,
    state: &JetWebTableState,
    columns: &[JetWebTableColumn<T>],
    row_key: Option<&JetWebTableRowKey<T>>,
) -> Result<JetWebTablePage<T>, String> {
    jet_web_table_validate_columns(columns)?;
    let mut selected = jet_web_table_rows_with_keys(rows, row_key)?;
    if let Some(filter) = &state.filter {
        let column = columns
            .iter()
            .find(|column| column.name == filter.column)
            .ok_or_else(|| format!("table filter column `{}` has no accessor", filter.column))?;
        selected.retain(|entry| column.value(&entry.value).contains(filter.value.as_str()));
    }
    if let Some(sort) = &state.sort {
        let column = columns
            .iter()
            .find(|column| column.name == sort.column)
            .ok_or_else(|| format!("table sort column `{}` has no accessor", sort.column))?;
        let descending = sort.direction.is_descending();
        // `sort_by` is stable. Equal cell values retain source order, while
        // the key remains attached to the row through the reorder.
        selected.sort_by(|left, right| {
            let ordering = column.value(&left.value).cmp(&column.value(&right.value));
            if descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
    }
    Ok(jet_web_table_page_keyed(
        &selected,
        state.page_index,
        state.page_size,
    ))
}

fn jet_web_table_page_state_changed(
    previous: &JetWebTableState,
    next: &JetWebTableState,
) -> bool {
    previous.sort != next.sort
        || previous.filter != next.filter
        || previous.page_index != next.page_index
        || previous.page_size != next.page_size
        || previous.page_mode != next.page_mode
}
fn jet_web_table_query_boundary_changed(
    previous: &JetWebTableState,
    next: &JetWebTableState,
) -> bool {
    previous.sort != next.sort
        || previous.filter != next.filter
        || previous.page_size != next.page_size
        || previous.page_mode != next.page_mode
}

fn jet_web_table_normalize_state(mut state: JetWebTableState) -> JetWebTableState {
    state.page_index = state.page_index.max(0);
    state.page_size = state
        .page_size
        .clamp(1, JET_WEB_TABLE_MAX_PAGE_SIZE);
    state.selected_keys.retain(|key| jet_web_table_key_is_valid(key));
    state.selected_keys.sort();
    state.selected_keys.dedup();
    if state
        .selection_anchor
        .as_ref()
        .is_some_and(|key| !state.selected_keys.iter().any(|selected| selected == key))
    {
        state.selection_anchor = None;
    }
    if state
        .focus_key
        .as_ref()
        .is_some_and(|key| !jet_web_table_key_is_valid(key))
    {
        state.focus_key = None;
    }
    state
}

fn jet_web_table_reconcile_state<T>(
    state: JetWebTableState,
    rows: &[T],
    row_key: Option<&JetWebTableRowKey<T>>,
) -> JetWebTableState {
    let mut state = jet_web_table_normalize_state(state);
    let Ok(keys) = jet_web_table_row_keys(rows, row_key) else {
        return state;
    };
    let keys = keys.into_iter().collect::<JetWebTableSet<_>>();
    state.selected_keys.retain(|key| keys.contains(key));
    if state
        .selection_anchor
        .as_ref()
        .is_some_and(|key| !keys.contains(key))
    {
        state.selection_anchor = None;
    }
    if state
        .focus_key
        .as_ref()
        .is_some_and(|key| !keys.contains(key))
    {
        state.focus_key = None;
    }
    state
}

fn jet_web_table_key_is_valid(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= JET_WEB_TABLE_MAX_KEY_BYTES
        && !key.chars().any(|character| character.is_control())
}

fn jet_web_table_validate_keys(keys: &[String]) -> Result<(), String> {
    let mut seen = JetWebTableSet::new();
    for key in keys {
        if !jet_web_table_key_is_valid(key) {
            return Err("table row key must be non-empty, bounded, and printable".to_string());
        }
        if !seen.insert(key.clone()) {
            return Err(format!("table row key `{key}` is duplicated"));
        }
    }
    Ok(())
}

fn jet_web_table_validate_columns<T>(
    columns: &[JetWebTableColumn<T>],
) -> Result<(), String> {
    let mut seen = JetWebTableSet::new();
    for column in columns {
        if !jet_web_table_key_is_valid(&column.name) {
            return Err("table column name must be non-empty, bounded, and printable".to_string());
        }
        if !jet_web_table_key_is_valid(&column.cell_type) {
            return Err("table column cell type must be non-empty, bounded, and printable".to_string());
        }
        if !seen.insert(column.name.clone()) {
            return Err(format!("table column `{}` is duplicated", column.name));
        }
    }
    Ok(())
}

fn jet_web_table_row_keys<T>(
    rows: &[T],
    row_key: Option<&JetWebTableRowKey<T>>,
) -> Result<Vec<String>, String> {
    let keys = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            row_key.map(|key| key(row)).unwrap_or_else(|| format!("index:{index}"))
        })
        .collect::<Vec<_>>();
    jet_web_table_validate_keys(&keys)?;
    Ok(keys)
}

fn jet_web_table_rows_with_keys<T: Clone>(
    rows: &[T],
    row_key: Option<&JetWebTableRowKey<T>>,
) -> Result<Vec<JetWebTableRow<T>>, String> {
    Ok(rows
        .iter()
        .cloned()
        .zip(jet_web_table_row_keys(rows, row_key)?)
        .map(|(value, key)| JetWebTableRow { value, key })
        .collect())
}

fn jet_web_table_page_keyed<T: Clone>(
    rows: &[JetWebTableRow<T>],
    page_index: i64,
    page_size: i64,
) -> JetWebTablePage<T> {
    let page_size = page_size.clamp(1, JET_WEB_TABLE_MAX_PAGE_SIZE);
    let total_rows = i64::try_from(rows.len()).unwrap_or(i64::MAX);
    let page_count = if total_rows == 0 {
        0
    } else {
        total_rows
            .saturating_add(page_size.saturating_sub(1))
            .checked_div(page_size)
            .unwrap_or(0)
    };
    let page_index = if page_count == 0 {
        0
    } else {
        page_index.max(0).min(page_count - 1)
    };
    let start = page_index
        .saturating_mul(page_size)
        .min(total_rows)
        .try_into()
        .unwrap_or(usize::MAX)
        .min(rows.len());
    let end = start
        .saturating_add(page_size.try_into().unwrap_or(usize::MAX))
        .min(rows.len());
    JetWebTablePage {
        rows: rows[start..end]
            .iter()
            .map(|row| row.value.clone())
            .collect(),
        row_keys: rows[start..end]
            .iter()
            .map(|row| row.key.clone())
            .collect(),
        total_rows,
        page_index,
        page_size,
        page_count,
    }
}

fn jet_web_table_string_list_json(values: &[String]) -> String {
    values
        .iter()
        .map(|value| format!("\"{}\"", jet_web_table_json_escape(value)))
        .collect::<Vec<_>>()
        .join(",")
}

fn jet_web_table_json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                escaped.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => escaped.push(character),
        }
    }
    escaped
}
