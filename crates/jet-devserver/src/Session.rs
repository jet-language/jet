//! Resident `jet dev` session state shared by Canvas and application views.
//!
//! The session is deliberately transport-neutral.  The Canvas listener and
//! the application listener both point at this state, while the application
//! remains responsible for its own routes and middleware.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use jet_foundation::JSON::{json_escape, parse_json, JSONValue};

const MAX_CLIENT_ID: usize = 128;
const MAX_RECEIPTS: usize = 128;
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

struct Receipt {
    kind: String,
    status: String,
    before: String,
    after: String,
    client: String,
    output: String,
}

/// One semantic resident development session.
///
/// Every field is session state, not browser state.  Browser views can
/// reconnect or multiply without creating another source history or another
/// accepted-revision stream.
pub struct ResidentDevSession {
    id: String,
    entry: String,
    canvas_port: u16,
    application_port: u16,
    current_revision: Mutex<String>,
    accepted_revision: Mutex<String>,
    last_good_revision: Mutex<String>,
    last_good_program: Mutex<String>,
    state: Mutex<String>,
    diagnostic_code: Mutex<String>,
    diagnostic: Mutex<String>,
    selected_output: Mutex<String>,
    selected_target: Mutex<String>,
    debugger_state: Mutex<String>,
    test_state: Mutex<String>,
    clients: Mutex<HashMap<String, Instant>>,
    receipts: Mutex<Vec<Receipt>>,
}

impl ResidentDevSession {
    pub fn new(entry: &str, canvas_port: u16, application_port: u16) -> Self {
        let serial = NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed);
        Self {
            id: format!("jet-session-{}-{}", std::process::id(), serial),
            entry: entry.to_string(),
            canvas_port,
            application_port,
            current_revision: Mutex::new(String::new()),
            accepted_revision: Mutex::new(String::new()),
            last_good_revision: Mutex::new(String::new()),
            last_good_program: Mutex::new(String::new()),
            state: Mutex::new("starting".to_string()),
            diagnostic_code: Mutex::new(String::new()),
            diagnostic: Mutex::new(String::new()),
            selected_output: Mutex::new(String::new()),
            selected_target: Mutex::new(String::new()),
            debugger_state: Mutex::new("idle".to_string()),
            test_state: Mutex::new("idle".to_string()),
            clients: Mutex::new(HashMap::new()),
            receipts: Mutex::new(Vec::new()),
        }
    }

    pub fn note_client(&self, client: &str) {
        if client.is_empty() || client.len() > MAX_CLIENT_ID {
            return;
        }
        self.clients
            .lock()
            .unwrap()
            .insert(client.to_string(), Instant::now());
    }

    pub fn drop_client(&self, client: &str) {
        self.clients.lock().unwrap().remove(client);
    }

    pub fn observe_source(&self, revision: &str) {
        if !revision.is_empty() {
            *self.current_revision.lock().unwrap() = revision.to_string();
        }
    }

    pub fn mark_building(&self) {
        *self.state.lock().unwrap() = "building".to_string();
    }

    pub fn mark_ready(&self) {
        *self.state.lock().unwrap() = "ready".to_string();
        self.diagnostic_code.lock().unwrap().clear();
        self.diagnostic.lock().unwrap().clear();
    }

    pub fn mark_error(&self, code: &str, diagnostic: &str) {
        *self.state.lock().unwrap() = "error".to_string();
        *self.diagnostic_code.lock().unwrap() = code.to_string();
        *self.diagnostic.lock().unwrap() = diagnostic.to_string();
    }

    pub fn mark_last_good(&self, revision: &str, program: &str) {
        if revision.is_empty() {
            return;
        }
        *self.last_good_revision.lock().unwrap() = revision.to_string();
        *self.last_good_program.lock().unwrap() = program.to_string();
    }

    pub fn accept_transaction(&self, request: &str, revision: &str) {
        let before = request_string(request, "revision");
        let client = request_string(request, "client_id");
        let kind = request_string(request, "op");
        self.observe_source(revision);
        if !revision.is_empty() {
            *self.accepted_revision.lock().unwrap() = revision.to_string();
        }
        self.push_receipt(Receipt {
            kind: if kind.is_empty() { "source".to_string() } else { kind },
            status: "accepted".to_string(),
            before,
            after: revision.to_string(),
            client,
            output: request_string(request, "output"),
        });
    }

    pub fn refuse_transaction(&self, request: &str, current_revision: &str) {
        self.push_receipt(Receipt {
            kind: request_string(request, "op"),
            status: "refused".to_string(),
            before: request_string(request, "revision"),
            after: current_revision.to_string(),
            client: request_string(request, "client_id"),
            output: request_string(request, "output"),
        });
    }

    pub fn record_command(&self, request: &str) {
        let action = request_string(request, "action_id");
        let output = request_string(request, "output");
        let target = request_string(request, "target");
        if !output.is_empty() {
            *self.selected_output.lock().unwrap() = output;
        }
        if !target.is_empty() {
            *self.selected_target.lock().unwrap() = target;
        }
        if action.contains("test") {
            *self.test_state.lock().unwrap() = "requested".to_string();
        }
        self.push_receipt(Receipt {
            kind: if action.is_empty() { "command".to_string() } else { action },
            status: "requested".to_string(),
            before: String::new(),
            after: self.current_revision.lock().unwrap().clone(),
            client: request_string(request, "client_id"),
            output: self.selected_output.lock().unwrap().clone(),
        });
    }

    pub fn record_debug(&self, request: &str) {
        let op = request_string(request, "op");
        *self.debugger_state.lock().unwrap() = if op == "stop" || op == "disconnect" {
            "idle".to_string()
        } else {
            "active".to_string()
        };
    }

    pub fn select_output(&self, request: &str) {
        let output = request_string(request, "output");
        let target = request_string(request, "target");
        if !output.is_empty() {
            *self.selected_output.lock().unwrap() = output;
        }
        if !target.is_empty() {
            *self.selected_target.lock().unwrap() = target;
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn json(&self) -> String {
        let current = self.current_revision.lock().unwrap().clone();
        let accepted = self.accepted_revision.lock().unwrap().clone();
        let last_good = self.last_good_revision.lock().unwrap().clone();
        let program = self.last_good_program.lock().unwrap().clone();
        let state = self.state.lock().unwrap().clone();
        let diagnostic_code = self.diagnostic_code.lock().unwrap().clone();
        let diagnostic = self.diagnostic.lock().unwrap().clone();
        let output = self.selected_output.lock().unwrap().clone();
        let target = self.selected_target.lock().unwrap().clone();
        let debugger = self.debugger_state.lock().unwrap().clone();
        let tests = self.test_state.lock().unwrap().clone();
        let clients = self.clients.lock().unwrap().len();
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
            "{{\"id\":{},\"entry\":{},\"source_revision\":{},\"accepted_revision\":{},\"last_good_revision\":{},\"last_good_program\":{},\"state\":{},\"diagnostic_code\":{},\"diagnostic\":{},\"clients\":{},\"run\":{{\"output\":{},\"target\":{}}},\"debugger\":{{\"state\":{}}},\"tests\":{{\"state\":{}}},\"history\":{{\"count\":{},\"receipts\":[{}]}},\"listeners\":{{\"canvas\":{{\"host\":\"127.0.0.1\",\"port\":{},\"transport\":\"canvas\"}},\"application\":{{\"host\":\"127.0.0.1\",\"port\":{},\"transport\":\"application\",\"routes\":\"application-owned\"}}}},\"custom_servers\":{{\"owner\":\"application\",\"transport\":\"application\",\"reload\":\"source-transaction\"}}}}",
            json_value(&self.id),
            json_value(&self.entry),
            json_value(&current),
            json_value(&accepted),
            json_value(&last_good),
            json_value(&program),
            json_value(&state),
            json_value(&diagnostic_code),
            json_value(&diagnostic),
            clients,
            json_value(&output),
            json_value(&target),
            json_value(&debugger),
            json_value(&tests),
            self.receipts.lock().unwrap().len(),
            receipts,
            self.canvas_port,
            self.application_port
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

#[cfg(test)]
mod tests {
    use super::ResidentDevSession;

    #[test]
    fn one_session_keeps_history_last_good_and_listener_boundaries() {
        let session = ResidentDevSession::new("app.jet", 8080, 49152);
        session.note_client("a");
        session.select_output(r#"{"output":"web","target":"browser","client_id":"a"}"#);
        session.accept_transaction(
            r#"{"op":"replace_source","revision":"old","client_id":"a","output":"web"}"#,
            "new",
        );
        session.mark_last_good("new", "web-build-2");
        let json = session.json();
        assert!(json.contains("\"canvas\":{\"host\":\"127.0.0.1\",\"port\":8080"));
        assert!(json.contains("\"application\":{\"host\":\"127.0.0.1\",\"port\":49152"));
        assert!(json.contains("\"accepted_revision\":\"new\""));
        assert!(json.contains("\"last_good_program\":\"web-build-2\""));
        assert!(json.contains("\"count\":1"));
        assert!(json.contains("\"status\":\"accepted\""));
    }
}
