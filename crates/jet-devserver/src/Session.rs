//! Resident `jet dev` session state shared by Canvas and application views.
//!
//! The session is deliberately transport-neutral.  The default web host
//! exposes separate Canvas and application listeners; the listener facets
//! below keep route ownership inspectable in the payload.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use jet_foundation::JSON::{json_escape, parse_json, JSONValue};

pub(crate) const MAX_CLIENT_ID: usize = 128;
pub(crate) const MAX_CLIENTS: usize = 256;
pub(crate) const CLIENT_TTL_MS: u64 = 160;
pub(crate) const CLIENT_TTL: Duration = Duration::from_millis(CLIENT_TTL_MS);
const MAX_RECEIPTS: usize = 128;
const MAX_RETAINED_VIEW_BYTES: usize = 2 * 1024 * 1024;
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

struct Receipt {
    kind: String,
    status: String,
    before: String,
    after: String,
    client: String,
    output: String,
}

#[derive(Clone)]
struct DebuggerSnapshot {
    state: String,
    session_id: String,
    source_id: String,
    revision: String,
    tier: String,
}

#[derive(Clone, Default)]
struct RetainedView {
    revision: String,
    source_id: String,
    payload: String,
}

/// One semantic resident development session.
///
/// Every field is session state, not browser state.  Browser views can
/// reconnect or multiply without creating another source history or another
/// accepted-revision stream.
pub struct ResidentDevSession {
    source_transactions: crate::WatchService::SessionBroker,
    id: String,
    entry: String,
    canvas_host: String,
    canvas_port: u16,
    application_host: String,
    application_port: u16,
    current_revision: Mutex<String>,
    accepted_revision: Mutex<String>,
    last_good_revision: Mutex<String>,
    last_good_program: Mutex<String>,
    state: Mutex<String>,
    diagnostic_code: Mutex<String>,
    diagnostic: Mutex<String>,
    diagnostic_revision: Mutex<String>,
    selected_source_id: Mutex<String>,
    selected_output: Mutex<String>,
    selected_target: Mutex<String>,
    debugger: Mutex<DebuggerSnapshot>,
    test_state: Mutex<String>,
    last_good_views: Mutex<HashMap<String, RetainedView>>,
    clients: Mutex<HashMap<String, Instant>>,
    receipts: Mutex<Vec<Receipt>>,
}

impl ResidentDevSession {
    pub fn new(entry: &str, canvas_port: u16, application_port: u16) -> Self {
        Self::new_with_canvas_host(entry, "127.0.0.1", canvas_port, application_port)
    }

    pub fn new_with_canvas_host(
        entry: &str,
        canvas_host: &str,
        canvas_port: u16,
        application_port: u16,
    ) -> Self {
        Self::new_with_hosts(
            entry,
            canvas_host,
            canvas_port,
            "127.0.0.1",
            application_port,
        )
    }

    pub(crate) fn new_with_hosts(
        entry: &str,
        canvas_host: &str,
        canvas_port: u16,
        application_host: &str,
        application_port: u16,
    ) -> Self {
        let serial = NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed);
        Self {
            source_transactions: crate::WatchService::SessionBroker::default(),
            id: format!("jet-session-{}-{}", std::process::id(), serial),
            entry: entry.to_string(),
            canvas_host: canvas_host.to_string(),
            canvas_port,
            application_host: application_host.to_string(),
            application_port,
            current_revision: Mutex::new(String::new()),
            accepted_revision: Mutex::new(String::new()),
            last_good_revision: Mutex::new(String::new()),
            last_good_program: Mutex::new(String::new()),
            state: Mutex::new("starting".to_string()),
            diagnostic_code: Mutex::new(String::new()),
            diagnostic: Mutex::new(String::new()),
            diagnostic_revision: Mutex::new(String::new()),
            selected_source_id: Mutex::new(String::new()),
            selected_output: Mutex::new(String::new()),
            selected_target: Mutex::new(String::new()),
            debugger: Mutex::new(DebuggerSnapshot {
                state: "idle".to_string(),
                session_id: String::new(),
                source_id: String::new(),
                revision: String::new(),
                tier: String::new(),
            }),
            test_state: Mutex::new("idle".to_string()),
            last_good_views: Mutex::new(HashMap::new()),
            clients: Mutex::new(HashMap::new()),
            receipts: Mutex::new(Vec::new()),
        }
    }

    pub fn note_client(&self, client: &str) {
        if !client_id_is_valid(client) {
            return;
        }
        let now = Instant::now();
        let mut clients = self.clients.lock().unwrap();
        expire_clients(&mut clients, now);
        if !clients.contains_key(client) && clients.len() >= MAX_CLIENTS {
            return;
        }
        clients.insert(client.to_string(), now);
    }

    pub fn drop_client(&self, client: &str) {
        if !client_id_is_valid(client) {
            return;
        }
        let now = Instant::now();
        let mut clients = self.clients.lock().unwrap();
        expire_clients(&mut clients, now);
        clients.remove(client);
    }

    pub fn observe_source(&self, revision: &str) {
        if revision.is_empty() {
            return;
        }
        *self.current_revision.lock().unwrap() = revision.to_string();
        let mut accepted = self.accepted_revision.lock().unwrap();
        if accepted.is_empty() {
            *accepted = revision.to_string();
        }
    }

    pub fn mark_building(&self) {
        *self.state.lock().unwrap() = "building".to_string();
    }

    pub fn mark_ready(&self) {
        *self.state.lock().unwrap() = "ready".to_string();
        self.diagnostic_code.lock().unwrap().clear();
        self.diagnostic.lock().unwrap().clear();
        self.diagnostic_revision.lock().unwrap().clear();
    }

    pub fn mark_error(&self, code: &str, diagnostic: &str) {
        *self.state.lock().unwrap() = "error".to_string();
        *self.diagnostic_code.lock().unwrap() = code.to_string();
        *self.diagnostic.lock().unwrap() = diagnostic.to_string();
        *self.diagnostic_revision.lock().unwrap() =
            self.current_revision.lock().unwrap().clone();
    }

    pub fn mark_last_good(&self, revision: &str, program: &str) {
        if revision.is_empty() {
            return;
        }
        *self.last_good_revision.lock().unwrap() = revision.to_string();
        *self.last_good_program.lock().unwrap() = program.to_string();
    }

    /// Retain the last successful payload for a view.  The payload is opaque
    /// to the broker: each view remains responsible for its own report shape,
    /// while reconnecting clients can recover the last-good projection after
    /// a failed source rebuild.
    pub fn remember_last_good_view(
        &self,
        kind: &str,
        source_id: &str,
        revision: &str,
        payload: &str,
    ) {
        if !matches!(kind, "graph" | "debugger" | "runtime")
            || revision.is_empty()
            || payload.is_empty()
            || payload.len() > MAX_RETAINED_VIEW_BYTES
        {
            return;
        }
        self.last_good_views.lock().unwrap().insert(
            kind.to_string(),
            RetainedView {
                revision: revision.to_string(),
                source_id: source_id.to_string(),
                payload: payload.to_string(),
            },
        );
    }

    pub(crate) fn last_good_view(
        &self,
        kind: &str,
        source_id: &str,
    ) -> Option<(String, String)> {
        let views = self.last_good_views.lock().unwrap();
        let view = views.get(kind)?;
        if view.source_id != source_id {
            return None;
        }
        Some((view.revision.clone(), view.payload.clone()))
    }

    fn clear_last_good_view(&self, kind: &str) {
        self.last_good_views.lock().unwrap().remove(kind);
    }

    pub fn accept_transaction(&self, request: &str, revision: &str) {
        let before = request_string(request, "revision");
        let client = request_string(request, "client_id");
        let kind = request_string(request, "op");
        self.select_project_source(&request_string(request, "source_id"));
        self.observe_source(revision);
        if !revision.is_empty() {
            *self.accepted_revision.lock().unwrap() = revision.to_string();
        }
        self.push_receipt(Receipt {
            kind: if kind.is_empty() {
                "source".to_string()
            } else {
                kind
            },
            status: "accepted".to_string(),
            before,
            after: revision.to_string(),
            client,
            output: request_string(request, "output"),
        });
    }

    pub fn refuse_transaction(&self, request: &str, current_revision: &str) {
        self.observe_source(current_revision);
        self.push_receipt(Receipt {
            kind: request_string(request, "op"),
            status: "refused".to_string(),
            before: request_string(request, "revision"),
            after: current_revision.to_string(),
            client: request_string(request, "client_id"),
            output: request_string(request, "output"),
        });
    }

    pub(crate) fn lock_source_transaction(&self) -> std::sync::MutexGuard<'_, ()> {
        self.source_transactions.lock_source_transaction()
    }

    pub fn record_command(&self, request: &str) {
        let action = request_string(request, "action_id");
        self.select_project_source(&request_string(request, "source_id"));
        if action.contains("test") {
            *self.test_state.lock().unwrap() = "requested".to_string();
        }
        self.push_receipt(Receipt {
            kind: if action.is_empty() {
                "command".to_string()
            } else {
                action
            },
            status: "requested".to_string(),
            before: String::new(),
            after: self.current_revision.lock().unwrap().clone(),
            client: request_string(request, "client_id"),
            output: self.selected_output.lock().unwrap().clone(),
        });
    }

    pub fn record_debug(&self, request: &str) {
        self.select_project_source(&request_string(request, "source_id"));
        let mut debugger = self.debugger.lock().unwrap();
        if request_bool(request, "stop") || request_string(request, "op") == "disconnect" {
            *debugger = idle_debugger();
            drop(debugger);
            self.clear_last_good_view("debugger");
            return;
        }
        debugger.state = "active".to_string();
        set_if_present(&mut debugger.session_id, &request_string(request, "session_id"));
        set_if_present(&mut debugger.source_id, &request_string(request, "source_id"));
        set_if_present(&mut debugger.revision, &request_string(request, "revision"));
        set_if_present(&mut debugger.tier, &request_string(request, "tier"));
    }

    pub fn record_debug_response(&self, request: &str, response: &str) {
        self.record_debug(request);
        if request_bool(request, "stop") {
            return;
        }
        let Some(snapshot) = find_debugger_snapshot(response) else {
            return;
        };
        let (source_id, debugger_revision) = {
            let mut debugger = self.debugger.lock().unwrap();
            let state = match snapshot.state.as_str() {
                "running" => "active".to_string(),
                "stopped" => "idle".to_string(),
                state => state.to_string(),
            };
            *debugger = DebuggerSnapshot {
                state,
                session_id: snapshot.session_id,
                source_id: snapshot.source_id,
                revision: snapshot.revision,
                tier: snapshot.tier,
            };
            let source_id = if debugger.source_id.is_empty() {
                request_string(request, "source_id")
            } else {
                debugger.source_id.clone()
            };
            (source_id, debugger.revision.clone())
        };
        let revision = if debugger_revision.is_empty() {
            let requested = request_string(request, "revision");
            if requested.is_empty() {
                self.current_revision.lock().unwrap().clone()
            } else {
                requested
            }
        } else {
            debugger_revision
        };
        self.remember_last_good_view("debugger", &source_id, &revision, response);
    }

    pub fn select_project_source(&self, source_id: &str) {
        if !source_id.is_empty() {
            *self.selected_source_id.lock().unwrap() = source_id.to_string();
        }
    }

    pub fn select_project_source_from_payload(&self, payload: &str) {
        if let Some(source_id) = json_string_any(payload, "source_id") {
            self.select_project_source(&source_id);
        }
    }

    pub fn select_output(&self, request: &str) {
        let output = request_string(request, "output");
        let target = request_string(request, "target");
        self.select_output_values(
            (!output.is_empty()).then_some(output.as_str()),
            (!target.is_empty()).then_some(target.as_str()),
        );
    }

    pub(crate) fn select_output_values(&self, output: Option<&str>, target: Option<&str>) {
        if let Some(output) = output.filter(|output| !output.is_empty()) {
            *self.selected_output.lock().unwrap() = output.to_string();
        }
        if let Some(target) = target.filter(|target| !target.is_empty()) {
            *self.selected_target.lock().unwrap() = target.to_string();
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn application_port(&self) -> u16 {
        self.application_port
    }

    pub fn canvas_port(&self) -> u16 {
        self.canvas_port
    }

    pub fn application_host(&self) -> &str {
        &self.application_host
    }

    pub fn json(&self) -> String {
        let current = self.current_revision.lock().unwrap().clone();
        let accepted = self.accepted_revision.lock().unwrap().clone();
        let last_good = self.last_good_revision.lock().unwrap().clone();
        let program = self.last_good_program.lock().unwrap().clone();
        let state = self.state.lock().unwrap().clone();
        let diagnostic_code = self.diagnostic_code.lock().unwrap().clone();
        let diagnostic = self.diagnostic.lock().unwrap().clone();
        let diagnostic_revision = self.diagnostic_revision.lock().unwrap().clone();
        let selected_source_id = self.selected_source_id.lock().unwrap().clone();
        let output = self.selected_output.lock().unwrap().clone();
        let target = self.selected_target.lock().unwrap().clone();
        let debugger = self.debugger.lock().unwrap().clone();
        let tests = self.test_state.lock().unwrap().clone();
        let views = self.last_good_views.lock().unwrap().clone();
        let now = Instant::now();
        let mut client_state = self.clients.lock().unwrap();
        expire_clients(&mut client_state, now);
        let clients = client_state.len();
        let mut listener_entries = Vec::new();
        if self.canvas_port != 0 {
            listener_entries.push(format!(
                "\"canvas\":{{\"host\":{},\"port\":{},\"transport\":\"canvas\",\"listener\":\"canvas\",\"shared\":false}}",
                json_value(&self.canvas_host),
                self.canvas_port,
            ));
        }
        if self.application_port != 0 {
            listener_entries.push(format!(
                "\"application\":{{\"host\":{},\"port\":{},\"transport\":\"application\",\"listener\":\"application\",\"routes\":\"application-owned\",\"shared\":false}}",
                json_value(&self.application_host),
                self.application_port,
            ));
        }
        let listeners = format!("{{{}}}", listener_entries.join(","));
        let custom_server_listener = if self.application_port != 0 {
            "\"application\""
        } else {
            "null"
        };
        let receipts = self
            .receipts
            .lock()
            .unwrap()
            .iter()
            .map(|receipt| {
                format!(
                    "{{\"kind\":{},\"status\":{},\"before\":{},\"after\":{},\"client\":{},\"output\":{}}}",
                    json_value(&receipt.kind),
                    json_value(&receipt.status),
                    json_value(&receipt.before),
                    json_value(&receipt.after),
                    json_value(&receipt.client),
                    json_value(&receipt.output)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"id\":{},\"entry\":{},\"project_context\":{{\"source_id\":{}}},\"source_revision\":{},\"accepted_revision\":{},\"last_good_revision\":{},\"last_good_program\":{},\"state\":{},\"diagnostic_code\":{},\"diagnostic\":{},\"diagnostic_revision\":{},\"last_good_views\":{{\"graph\":{},\"debugger\":{},\"runtime\":{}}},\"clients\":{},\"run\":{{\"output\":{},\"target\":{}}},\"debugger\":{{\"state\":{},\"session_id\":{},\"source_id\":{},\"revision\":{},\"tier\":{}}},\"tests\":{{\"state\":{}}},\"history\":{{\"count\":{},\"receipts\":[{}]}},\"listeners\":{},\"custom_servers\":{{\"owner\":\"application\",\"transport\":\"application\",\"reload\":\"source-transaction\",\"listener\":{}}}}}",
            json_value(&self.id),
            json_value(&self.entry),
            json_value(&selected_source_id),
            json_value(&current),
            json_value(&accepted),
            json_value(&last_good),
            json_value(&program),
            json_value(&state),
            json_value(&diagnostic_code),
            json_value(&diagnostic),
            json_value(&diagnostic_revision),
            retained_view_json(views.get("graph")),
            retained_view_json(views.get("debugger")),
            retained_view_json(views.get("runtime")),
            clients,
            json_value(&output),
            json_value(&target),
            json_value(&debugger.state),
            json_value(&debugger.session_id),
            json_value(&debugger.source_id),
            json_value(&debugger.revision),
            json_value(&debugger.tier),
            json_value(&tests),
            self.receipts.lock().unwrap().len(),
            receipts,
            listeners,
            custom_server_listener
        )
    }

    fn push_receipt(&self, receipt: Receipt) {
        let mut receipts = self.receipts.lock().unwrap();
        receipts.push(receipt);
        if receipts.len() > MAX_RECEIPTS {
            receipts.remove(0);
        }
    }
}

fn expire_clients(clients: &mut HashMap<String, Instant>, now: Instant) {
    clients.retain(|_, seen| now.saturating_duration_since(*seen) <= CLIENT_TTL);
}

pub(crate) fn client_id_is_valid(client: &str) -> bool {
    !client.is_empty() && client.len() <= MAX_CLIENT_ID && !client.chars().any(char::is_control)
}

fn retained_view_json(view: Option<&RetainedView>) -> String {
    let view = view.cloned().unwrap_or_default();
    format!(
        "{{\"revision\":{},\"source_id\":{},\"payload\":{}}}",
        json_value(&view.revision),
        json_value(&view.source_id),
        json_value(&view.payload)
    )
}

fn json_value(value: &str) -> String {
    if value.is_empty() {
        "null".to_string()
    } else {
        format!("\"{}\"", json_escape(value))
    }
}

fn request_string(text: &str, key: &str) -> String {
    let Ok(JSONValue::Object(object)) = parse_json(text) else {
        return String::new();
    };
    object
        .get(key)
        .and_then(|value| match value {
            JSONValue::String(value) => Some(value.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

fn request_bool(text: &str, key: &str) -> bool {
    let Ok(JSONValue::Object(object)) = parse_json(text) else {
        return false;
    };
    matches!(object.get(key), Some(JSONValue::Bool(true)))
}

fn set_if_present(target: &mut String, value: &str) {
    if !value.is_empty() {
        *target = value.to_string();
    }
}

fn idle_debugger() -> DebuggerSnapshot {
    DebuggerSnapshot {
        state: "idle".to_string(),
        session_id: String::new(),
        source_id: String::new(),
        revision: String::new(),
        tier: String::new(),
    }
}

fn json_string_any(text: &str, key: &str) -> Option<String> {
    let value = parse_json(text).ok()?;

    fn find(value: &JSONValue, key: &str) -> Option<String> {
        match value {
            JSONValue::Object(object) => {
                if let Some(JSONValue::String(value)) = object.get(key) {
                    return Some(value.clone());
                }
                object.values().find_map(|value| find(value, key))
            }
            JSONValue::Array(values) => values.iter().find_map(|value| find(value, key)),
            _ => None,
        }
    }

    find(&value, key)
}

fn find_debugger_snapshot(text: &str) -> Option<DebuggerSnapshot> {
    let value = parse_json(text).ok()?;

    fn find(value: &JSONValue) -> Option<DebuggerSnapshot> {
        match value {
            JSONValue::Object(object) => {
                if let Some(JSONValue::String(session_id)) = object.get("id") {
                    if session_id.starts_with("canvas-debug-") {
                        let field = |key: &str| {
                            object
                                .get(key)
                                .and_then(|value| match value {
                                    JSONValue::String(value) => Some(value.clone()),
                                    _ => None,
                                })
                                .unwrap_or_default()
                        };
                        return Some(DebuggerSnapshot {
                            state: field("state"),
                            session_id: session_id.clone(),
                            source_id: field("source_id"),
                            revision: field("revision"),
                            tier: field("tier"),
                        });
                    }
                }
                object.values().find_map(find)
            }
            JSONValue::Array(values) => values.iter().find_map(find),
            _ => None,
        }
    }

    find(&value)
}

#[cfg(test)]
mod tests {
    use super::{ResidentDevSession, CLIENT_TTL, MAX_CLIENTS};
    use std::time::Instant;

    #[test]
    fn one_session_keeps_history_last_good_and_listener_boundaries() {
        let session = ResidentDevSession::new("app.jet", 8080, 49152);
        session.note_client("a");
        session.note_client("b");
        session.select_output(r#"{"output":"web","target":"browser","client_id":"a"}"#);
        session.accept_transaction(
            r#"{"op":"replace_source","revision":"old","client_id":"a","output":"web"}"#,
            "new",
        );
        session.mark_last_good("new", "web-build-2");
        let json = session.json();
        assert!(json.contains("\"canvas\":{\"host\":\"127.0.0.1\",\"port\":8080"));
        assert!(json.contains("\"application\":{\"host\":\"127.0.0.1\",\"port\":49152"));
        assert!(json.contains("\"listener\":\"canvas\""));
        assert!(json.contains("\"listener\":\"application\""));
        assert!(!json.contains("\"shared\":true"));
        assert!(json.contains("\"accepted_revision\":\"new\""));
        assert!(json.contains("\"last_good_program\":\"web-build-2\""));
        assert!(json.contains("\"clients\":2"));
        assert!(json.contains("\"run\":{\"output\":\"web\",\"target\":\"browser\"}"));
        assert!(json.contains("\"count\":1"));
        assert!(json.contains("\"status\":\"accepted\""));
    }

    #[test]
    fn session_json_lists_only_bound_listeners() {
        let app = ResidentDevSession::new("app.jet", 0, 49152).json();
        assert!(app.contains("\"listeners\":{\"application\""));
        assert!(!app.contains("\"listeners\":{\"canvas\""));
        assert!(!app.contains("\"workbench\""));

        let canvas = ResidentDevSession::new("app.jet", 4567, 0).json();
        assert!(canvas.contains("\"listeners\":{\"canvas\""));
        assert!(!canvas.contains("\"listeners\":{\"application\""));
        assert!(!canvas.contains("\"workbench\""));
    }

    #[test]
    fn canvas_command_metadata_does_not_change_program_selection() {
        let session = ResidentDevSession::new("app.jet", 4567, 49152);
        session.record_command(
            r#"{"action_id":"canvas.command:run","output":"native","target":"desktop","client_id":"b"}"#,
        );
        let json = session.json();
        assert!(json.contains("\"canvas\":{\"host\":\"127.0.0.1\",\"port\":4567"));
        assert!(json.contains("\"run\":{\"output\":null,\"target\":null}"));
    }

    #[test]
    fn failed_rebuild_keeps_last_good_views_and_current_source_diagnostics() {
        let session = ResidentDevSession::new("app.jet", 4567, 49152);
        session.observe_source("good-revision");
        session.mark_last_good("good-revision", "web-build-1");
        session.remember_last_good_view("graph", "", "good-revision", "graph-good");
        session.remember_last_good_view("debugger", "", "good-revision", "debug-good");
        session.remember_last_good_view("runtime", "", "good-revision", "runtime-good");

        session.observe_source("broken-revision");
        session.mark_error("E0102", "Error [E0102]: missing name");
        let json = session.json();

        for expected in [
            "\"source_revision\":\"broken-revision\"",
            "\"accepted_revision\":\"good-revision\"",
            "\"last_good_revision\":\"good-revision\"",
            "\"last_good_program\":\"web-build-1\"",
            "\"last_good_views\":{\"graph\":{\"revision\":\"good-revision\",\"source_id\":null,\"payload\":\"graph-good\"},\"debugger\":{\"revision\":\"good-revision\",\"source_id\":null,\"payload\":\"debug-good\"},\"runtime\":{\"revision\":\"good-revision\",\"source_id\":null,\"payload\":\"runtime-good\"}}",
            "\"diagnostic_revision\":\"broken-revision\"",
            "\"state\":\"error\"",
            "\"diagnostic_code\":\"E0102\"",
            "Error [E0102]: missing name",
        ] {
            assert!(json.contains(expected), "session lost {expected}: {json}");
        }
    }

    #[test]
    fn first_observed_source_is_the_accepted_session_baseline() {
        let session = ResidentDevSession::new("app.jet", 4567, 49152);
        session.observe_source("initial-revision");
        session.observe_source("current-revision");
        let json = session.json();
        assert!(json.contains("\"source_revision\":\"current-revision\""));
        assert!(json.contains("\"accepted_revision\":\"initial-revision\""));
    }

    #[test]
    fn disconnect_reconnect_preserves_project_run_debug_and_last_good_state() {
        let session = ResidentDevSession::new("app.jet", 4567, 49152);
        session.note_client("canvas-a");
        session.select_output(r#"{"output":"web","target":"browser"}"#);
        session.accept_transaction(
            r#"{"op":"replace_source","source_id":"helper.jet","revision":"old","client_id":"canvas-a"}"#,
            "accepted-revision",
        );
        session.mark_last_good("accepted-revision", "web-build-2");
        session.record_debug_response(
            r#"{"schema_version":1,"revision":"accepted-revision","source_id":"helper.jet","commands":["s"]}"#,
            r#"{"schema":"jet.report/v1","canvas":{"protocol":"jet.canvas.debug","session":{"id":"canvas-debug-1","state":"running","tier":"jet-dev-interpreter","source_id":"helper.jet","revision":"accepted-revision"}}}"#,
        );

        session.drop_client("canvas-a");
        session.note_client("canvas-b");
        let json = session.json();
        for expected in [
            "\"project_context\":{\"source_id\":\"helper.jet\"}",
            "\"accepted_revision\":\"accepted-revision\"",
            "\"last_good_program\":\"web-build-2\"",
            "\"run\":{\"output\":\"web\",\"target\":\"browser\"}",
            "\"debugger\":{\"state\":\"active\",\"session_id\":\"canvas-debug-1\"",
            "\"clients\":1",
        ] {
            assert!(json.contains(expected), "reconnect lost {expected}: {json}");
        }

        session.record_debug(
            r#"{"schema_version":1,"revision":"accepted-revision","source_id":"helper.jet","session_id":"canvas-debug-1","stop":true}"#,
        );
        assert!(session.json().contains("\"debugger\":{\"state\":\"idle\",\"session_id\":null"));
    }

    #[test]
    fn client_registry_is_bounded_and_expires_before_admission() {
        let session = ResidentDevSession::new("app.jet", 4567, 49152);
        for index in 0..MAX_CLIENTS {
            session.note_client(&format!("client-{index}"));
        }
        session.note_client("overflow");
        assert!(session.json().contains(&format!("\"clients\":{MAX_CLIENTS}")));

        let expired = Instant::now()
            .checked_sub(CLIENT_TTL + std::time::Duration::from_millis(1))
            .expect("test instant must have a past value");
        session.clients.lock().unwrap().insert("expired".into(), expired);
        session.note_client("replacement");
        assert!(session.json().contains("\"clients\":256"));
    }
}
