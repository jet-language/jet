//! Resident `jet dev` session state shared by Canvas and application views.
//!
//! The session is deliberately transport-neutral.  The default web host
//! exposes separate Canvas and application listeners; the listener facets
//! below keep route ownership inspectable in the payload.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::Devtools::{
    JetDevtoolsDatabasePanelFacts, JetDevtoolsHostKind, JetDevtoolsJobPanelFact,
    JetDevtoolsPanelCapability, JetDevtoolsPanelCatalog, JetDevtoolsPanelCatalogError,
    JetDevtoolsPanelProjection, JetDevtoolsRequestPanelFact, JetDevtoolsRequestPanelState,
    JetDevtoolsTelemetryPanelState, JetDevtoolsTopologyFact, JetDevtoolsTopologyState,
};

use jet_foundation::Devtools::{
    JetDevtoolsEnvelope, JetDevtoolsEvent, JetDevtoolsEventSink, JetDevtoolsEventSinkGuard,
    JetDevtoolsFreshnessFact, JetDevtoolsLifecycleState, JetDevtoolsSelection,
    JetDevtoolsSourceIdentityFact, JetDevtoolsTimeCursor, JET_DEVTOOLS_MAX_ENVELOPE_BYTES,
    JET_DEVTOOLS_MAX_TEXT_BYTES,
};
use jet_foundation::DevtoolsControl::{
    JetDevtoolsCommand, JetDevtoolsCommandEnvelope, JetDevtoolsCommandReceipt,
    JetDevtoolsDatabaseExplainRequest, JetDevtoolsDatabaseExplainResult,
    JetDevtoolsDecodedEnvelope, JetDevtoolsGameControlRequest,
    JetDevtoolsProjectRebuildRequest, JET_DEVTOOLS_MAX_COMMANDS,
};
use crate::SessionRecording::{
    RecordingError, ReplayCursor, ReplayProjection, SessionCheckpoint, SessionEvent, SessionIdentity,
    SessionRecording, TruncationFacts,
};


use jet_foundation::HotSwap::{HotSwapDecision, StateRetentionDecision};
use jet_foundation::DataTree::DataTree;
use jet_foundation::JSON::{json_escape, parse_json, parse_json_with_limit};
use jet_foundation::Persist::PersistRejectReason;
use crate::CapturePolicy::{CaptureFaultFact, CaptureIdentity, CaptureSkip, CaptureStartFact};
use crate::WatchService::{HotReplaceTxn, PersistOutcome, SessionSnapshot};
const DEVTOOLS_TEXT_LIMIT: usize = JET_DEVTOOLS_MAX_TEXT_BYTES;
pub(crate) const MAX_CLIENT_ID: usize = 128;
pub(crate) const MAX_CLIENTS: usize = 256;
pub(crate) const CLIENT_TTL_MS: u64 = 160;
pub(crate) const CLIENT_TTL: Duration = Duration::from_millis(CLIENT_TTL_MS);
const MAX_RECEIPTS: usize = 128;
const MAX_RETAINED_VIEW_BYTES: usize = 2 * 1024 * 1024;
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);
pub use jet_foundation::Devtools::JET_DEVTOOLS_PROTOCOL;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveLineage {
    pub protocol: String,
    pub session_id: String,
    pub source_id: String,
    pub build_id: String,
    pub revision: String,
    pub world_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveValueFact {
    pub value_id: String,
    pub type_identity: String,
    pub disposition: String,
    pub reason: String,
}

/// Typed capture lifecycle facts retained by the resident session.
///
/// A skip is emitted from the start decision rather than reconstructed from
/// transport JSON.  Faults retain the same start eligibility and identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CaptureSessionFact {
    Start(CaptureStartFact),
    Skip(CaptureSkip),
    Fault(CaptureFaultFact),
}

struct LiveReceipt {
    lineage: LiveLineage,
    values: Vec<LiveValueFact>,
}

struct Receipt {
    kind: String,
    status: String,
    before: String,
    after: String,
    client: String,
    output: String,
    live: Option<LiveReceipt>,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DevtoolsSelectionFact {
    pub panel_id: String,
    pub item_key: String,
}

pub use jet_foundation::Devtools::JetDevtoolsLifecycleState as DevtoolsSessionState;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DevtoolsStatusFact {
    pub state: DevtoolsSessionState,
    pub diagnostic_code: Option<String>,
    pub diagnostic: Option<String>,
    pub accepted_revision: Option<String>,
    pub last_good_revision: Option<String>,
    pub run_target: Option<String>,
    pub run_output: Option<String>,
    pub test_state: Option<String>,
}

/// Command capabilities are explicit host facts.  A browser may render a
/// reason when the resident driver is not currently able to consume a command;
/// it must not infer support from a panel's presence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DevtoolsCommandCapabilities {
    pub project_rebuild: bool,
    pub project_rebuild_reason: Option<String>,
}

pub struct ProjectRebuildExecutorGuard {
    available: Arc<Mutex<bool>>,
}

impl Drop for ProjectRebuildExecutorGuard {
    fn drop(&mut self) {
        if let Ok(mut available) = self.available.lock() {
            *available = false;
        }
    }
}


/// Host-neutral, reconnect-safe view of the bounded devtools stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DevtoolsProjection {
    pub protocol: String,
    pub session_id: String,
    pub source_id: Option<String>,
    pub build_id: Option<String>,
    pub revision: String,
    pub world_id: Option<String>,
    pub sequence: u64,
    pub observed_at: u64,
    pub panels: Vec<JetDevtoolsEvent>,
    pub selection: Option<DevtoolsSelectionFact>,
    pub status: DevtoolsStatusFact,
    pub command_capabilities: DevtoolsCommandCapabilities,
    pub command_receipts: Vec<JetDevtoolsCommandReceipt>,
    pub reset: bool,
    pub truncation: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DevtoolsRecordingDivergence {
    pub divergent: bool,
    pub reason: Option<String>,
    pub identity: SessionIdentity,
    pub expected_sequence: Option<u64>,
    pub actual_sequence: Option<u64>,
    pub retained_events: usize,
    pub dropped_events: u64,
    pub truncation: TruncationFacts,
}

struct ResidentRecordingState {
    recording: SessionRecording,
    cursor: ReplayCursor,
    checkpoints: HashMap<String, SessionCheckpoint>,
    first_sequence: Option<u64>,
}

impl ResidentRecordingState {
    fn new(recording: SessionRecording, first_sequence: Option<u64>) -> Self {
        let cursor = recording.replay();
        Self {
            recording,
            cursor,
            checkpoints: HashMap::new(),
            first_sequence,
        }
    }
}

const BROWSER_ASSET_WATCH_MAX_FILES: usize = 4096;
const BROWSER_ASSET_WATCH_MAX_EVENTS: usize = 256;
const BROWSER_ASSET_WATCH_MAX_PATH_BYTES: usize = 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
struct BrowserAssetStamp {
    modified: Option<SystemTime>,
    len: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BrowserAssetChange {
    path: PathBuf,
    kind: &'static str,
}

struct BrowserAssetWatcher {
    root: PathBuf,
    known: BTreeMap<PathBuf, BrowserAssetStamp>,
}

impl BrowserAssetWatcher {
    fn for_entry(entry: &str) -> Self {
        let entry = Path::new(entry);
        let parent = entry.parent().unwrap_or_else(|| Path::new("."));
        let root = parent.join("assets");
        let mut watcher = Self {
            root,
            known: BTreeMap::new(),
        };
        watcher.snapshot();
        watcher
    }

    fn reopen(&mut self, entry: &str) {
        let entry = Path::new(entry);
        let parent = entry.parent().unwrap_or_else(|| Path::new("."));
        self.root = parent.join("assets");
        self.snapshot();
    }

    fn stamp(path: &Path) -> Option<BrowserAssetStamp> {
        let metadata = fs::symlink_metadata(path).ok()?;
        if !metadata.file_type().is_file() {
            return None;
        }
        Some(BrowserAssetStamp {
            modified: metadata.modified().ok(),
            len: metadata.len(),
        })
    }

    fn collect(
        path: &Path,
        files: &mut BTreeMap<PathBuf, BrowserAssetStamp>,
        skipped: &mut usize,
    ) {
        if files.len() >= BROWSER_ASSET_WATCH_MAX_FILES {
            *skipped = skipped.saturating_add(1);
            return;
        }
        let Ok(metadata) = fs::symlink_metadata(path) else {
            return;
        };
        if metadata.file_type().is_symlink() {
            *skipped = skipped.saturating_add(1);
            return;
        }
        if metadata.file_type().is_file() {
            if let Some(stamp) = Self::stamp(path) {
                if path.to_string_lossy().len() <= BROWSER_ASSET_WATCH_MAX_PATH_BYTES {
                    files.insert(path.to_path_buf(), stamp);
                } else {
                    *skipped = skipped.saturating_add(1);
                }
            }
            return;
        }
        if !metadata.file_type().is_dir() {
            *skipped = skipped.saturating_add(1);
            return;
        }
        let Ok(entries) = fs::read_dir(path) else {
            *skipped = skipped.saturating_add(1);
            return;
        };
        let mut children = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        children.sort();
        for child in children {
            Self::collect(&child, files, skipped);
            if files.len() >= BROWSER_ASSET_WATCH_MAX_FILES {
                break;
            }
        }
    }

    fn snapshot(&mut self) {
        let mut files = BTreeMap::new();
        let mut _skipped = 0;
        Self::collect(&self.root, &mut files, &mut _skipped);
        self.known = files;
    }

    fn poll(&mut self) -> (Vec<BrowserAssetChange>, usize) {
        let mut current = BTreeMap::new();
        let mut skipped = 0;
        Self::collect(&self.root, &mut current, &mut skipped);
        let mut changes = Vec::new();
        for (path, stamp) in &current {
            let kind = match self.known.get(path) {
                None => "created",
                Some(previous) if previous != stamp => "changed",
                Some(_) => continue,
            };
            changes.push(BrowserAssetChange {
                path: path.clone(),
                kind,
            });
        }
        for path in self.known.keys() {
            if !current.contains_key(path) {
                changes.push(BrowserAssetChange {
                    path: path.clone(),
                    kind: "deleted",
                });
            }
        }
        let omitted = changes.len().saturating_sub(BROWSER_ASSET_WATCH_MAX_EVENTS);
        skipped = skipped.saturating_add(omitted);
        changes.truncate(BROWSER_ASSET_WATCH_MAX_EVENTS);
        self.known = current;
        (changes, skipped)
    }

    fn frame(&mut self, session_id: &str) -> String {
        let (changes, skipped) = self.poll();
        let events = changes
            .into_iter()
            .filter_map(|change| {
                let logical = change.path.strip_prefix(&self.root).ok()?;
                let logical = logical.to_string_lossy().replace('\\', "/");
                if logical.is_empty() {
                    return None;
                }
                Some(format!(
                    "{{\"root\":{{\"id\":\"game\",\"path\":\".\"}},\"logical_path\":\"{}\",\"kind\":\"{}\"}}",
                    json_escape(&format!("assets/{logical}")),
                    json_escape(change.kind),
                ))
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"protocol\":\"jet.devtools.v1\",\"session_id\":\"{}\",\"started_at_ms\":{},\"direction\":\"host_to_runtime\",\"events\":[{}],\"skipped\":{}}}",
            json_escape(session_id),
            unix_time_ms(),
            events,
            skipped,
        )
    }
}

struct DevtoolsState {
    envelope: JetDevtoolsEnvelope,
}

impl DevtoolsState {
    fn new(session_id: &str) -> Self {
        Self {
            envelope: JetDevtoolsEnvelope::new(session_id, unix_time_ms()),
        }
    }
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
    live_lineage: Mutex<Option<LiveLineage>>,
    live_values: Mutex<Vec<LiveValueFact>>,
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
    capture_facts: Mutex<Vec<CaptureSessionFact>>,
    last_good_views: Mutex<HashMap<String, RetainedView>>,
    clients: Mutex<HashMap<String, Instant>>,
    receipts: Mutex<Vec<Receipt>>,
    devtools: Mutex<DevtoolsState>,
    /// Browser game controls stay in the resident session until the running
    /// Wasm guest polls the canonical host-to-runtime envelope. They are not
    /// interpreted or reimplemented by this host adapter.
    browser_game_controls: Mutex<VecDeque<JetDevtoolsGameControlRequest>>,
    browser_asset_watcher: Mutex<BrowserAssetWatcher>,
    recording: Mutex<Option<ResidentRecordingState>>,
    project_rebuild_executor: Arc<Mutex<bool>>,
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
        let id = format!("jet-session-{}-{}", std::process::id(), serial);
        Self {
            source_transactions: crate::WatchService::SessionBroker::default(),
            id: id.clone(),
            entry: entry.to_string(),
            canvas_host: canvas_host.to_string(),
            canvas_port,
            application_host: application_host.to_string(),
            application_port,
            current_revision: Mutex::new(String::new()),
            live_lineage: Mutex::new(None),
            live_values: Mutex::new(Vec::new()),
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
            capture_facts: Mutex::new(Vec::new()),
            last_good_views: Mutex::new(HashMap::new()),
            clients: Mutex::new(HashMap::new()),
            receipts: Mutex::new(Vec::new()),
            devtools: Mutex::new(DevtoolsState::new(&id)),
            browser_game_controls: Mutex::new(VecDeque::new()),
            browser_asset_watcher: Mutex::new(BrowserAssetWatcher::for_entry(entry)),
            recording: Mutex::new(None),
            project_rebuild_executor: Arc::new(Mutex::new(false)),
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
    /// Return one bounded host-to-runtime game-control frame for the browser
    /// guest. The request objects were already admitted by the canonical
    /// Foundation decoder; this method only serializes the transport envelope.
    pub fn browser_game_control_frame(&self) -> Result<String, String> {
        const MAX_CONTROLS_PER_FRAME: usize = 32;
        let mut queued = self.browser_game_controls.lock().unwrap();
        let mut frame = JetDevtoolsCommandEnvelope::new(self.id.clone(), unix_time_ms());
        let mut selected = Vec::new();
        for _ in 0..MAX_CONTROLS_PER_FRAME {
            let Some(request) = queued.pop_front() else {
                break;
            };
            frame.enqueue_command(JetDevtoolsCommand::GameControl(request.clone()))?;
            selected.push(request);
        }
        match frame.serialize() {
            Ok(body) => {
                for request in &selected {
                    self.update_command_receipt(&request.request_id, "relayed", None);
                }
                Ok(body)
            }
            Err(error) => {
                for request in selected.into_iter().rev() {
                    queued.push_front(request);
                }
                Err(error)
            }
        }
    }

    /// Poll the declared server-side asset root without following symlinks.
    /// The browser receives observations only; canonical GameDevSession
    /// validates, coalesces, and publishes their meaning in Wasm.
    pub fn browser_game_asset_frame(&self) -> Result<String, String> {
        Ok(self
            .browser_asset_watcher
            .lock()
            .map_err(|_| "browser asset watcher is poisoned".to_string())?
            .frame(&self.id))
    }

    /// Reset the browser asset baseline when the resident game session is
    /// reopened.  Reopening observes the current declared root as the new
    /// baseline; it does not manufacture a change event from the restart.
    pub fn reopen_game_asset_watcher(&self, entry: &str) -> Result<(), String> {
        if entry.trim().is_empty() {
            return Err("browser game asset watcher entry must not be empty".to_string());
        }
        self.browser_asset_watcher
            .lock()
            .map_err(|_| "browser asset watcher is poisoned".to_string())?
            .reopen(entry);
        Ok(())
    }

    /// Return canonical typed events after `cursor` without making hosts parse
    /// `devtools_json`. A missing cursor requests the retained stream as a
    /// complete reset. A cursor older than the bounded history also requests
    /// reset and marks truncation so a host can discard stale panels.
    pub fn devtools_events_since(
        &self,
        cursor: Option<JetDevtoolsTimeCursor>,
    ) -> DevtoolsProjection {
        let current_revision = self.current_revision.lock().unwrap().clone();
        let observed_at = unix_time_ms();
        let state = self.devtools_lock();
        let envelope = &state.envelope;
        let source_identity = envelope.source_identity.clone();
        let revision = source_identity
            .as_ref()
            .and_then(|identity| identity.revision.clone())
            .unwrap_or(current_revision);
        let latest = envelope.latest_sequence().unwrap_or(0);
        let oldest = envelope.oldest_sequence();
        let truncation = envelope.truncated
            || cursor.is_some_and(|cursor| {
                oldest.is_some_and(|oldest| cursor.saturating_add(1) < oldest)
            });
        let reset = envelope.reset || cursor.is_none() || truncation;
        let panels = envelope
            .events_since(cursor)
            .cloned()
            .collect();
        let selection = envelope
            .view()
            .selection()
            .map(|selection| DevtoolsSelectionFact {
                panel_id: selection.panel_id.clone(),
                item_key: selection.item_key.clone().unwrap_or_default(),
            });
        let diagnostic_code = self.diagnostic_code.lock().unwrap().clone();
        let diagnostic = self.diagnostic.lock().unwrap().clone();
        let accepted_revision = self.accepted_revision.lock().unwrap().clone();
        let last_good_revision = self.last_good_revision.lock().unwrap().clone();
        let run_target = self.selected_target.lock().unwrap().clone();
        let run_output = self.selected_output.lock().unwrap().clone();
        let test_state = self.test_state.lock().unwrap().clone();
        let status = DevtoolsStatusFact {
            state: envelope.lifecycle,
            diagnostic_code: optional_fact(&diagnostic_code),
            diagnostic: optional_fact(&diagnostic),
            accepted_revision: optional_fact(&accepted_revision),
            last_good_revision: optional_fact(&last_good_revision),
            run_target: optional_fact(&run_target),
            run_output: optional_fact(&run_output),
            test_state: optional_fact(&test_state),
        };
        let command_capabilities = self.devtools_command_capabilities();
        let command_receipts = self.devtools_command_receipts();
        DevtoolsProjection {
            protocol: JET_DEVTOOLS_PROTOCOL.to_string(),
            session_id: self.id.clone(),
            source_id: source_identity
                .as_ref()
                .and_then(|identity| identity.source_id.clone()),
            build_id: source_identity
                .as_ref()
                .and_then(|identity| identity.build_id.clone()),
            revision,
            world_id: source_identity
                .as_ref()
                .and_then(|identity| identity.world_id.clone()),
            sequence: latest,
            observed_at,
            panels,
            selection,
            status,
            command_capabilities,
            command_receipts,
            reset,
            truncation,
        }
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

    /// Install the complete identity required by the `jet.live.v1` snapshot.
    /// Empty or control-bearing values remain unavailable rather than being
    /// replaced by generated defaults.
    pub fn set_live_lineage(
        &self,
        source_id: &str,
        build_id: &str,
        revision: &str,
        world_id: &str,
    ) -> Result<(), String> {
        if [source_id, build_id, revision, world_id]
            .iter()
            .any(|value| value.is_empty() || value.chars().any(char::is_control))
        {
            return Err("live lineage is incomplete".to_string());
        }
        *self.live_lineage.lock().unwrap() = Some(LiveLineage {
            protocol: "jet.live.v1".to_string(),
            session_id: self.id.clone(),
            source_id: source_id.to_string(),
            build_id: build_id.to_string(),
            revision: revision.to_string(),
            world_id: world_id.to_string(),
        });
        self.observe_source(revision);
        self.set_devtools_source_identity(
            Some(source_id.to_string()),
            Some(build_id.to_string()),
            Some(revision.to_string()),
            Some(world_id.to_string()),
        )?;
        Ok(())
    }

    /// Retain one typed capture start decision only when it belongs to this
    /// session's currently installed live lineage.
    pub fn record_capture_start(&self, fact: &CaptureStartFact) -> Result<(), String> {
        self.validate_capture_identity(&fact.identity)?;
        let mut facts = self.capture_facts.lock().unwrap();
        facts.push(CaptureSessionFact::Start(fact.clone()));
        if let Some(skip) = fact.eligibility.skip() {
            facts.push(CaptureSessionFact::Skip(skip.clone()));
        }
        trim_capture_facts(&mut facts);
        Ok(())
    }

    /// Retain a typed capture fault under the exact start/live identity.
    pub fn record_capture_fault(&self, fact: &CaptureFaultFact) -> Result<(), String> {
        self.validate_capture_identity(&fact.identity)?;
        let mut facts = self.capture_facts.lock().unwrap();
        facts.push(CaptureSessionFact::Fault(fact.clone()));
        trim_capture_facts(&mut facts);
        Ok(())
    }

    pub fn capture_facts(&self) -> Vec<CaptureSessionFact> {
        self.capture_facts.lock().unwrap().clone()
    }

    pub fn capture_skips(&self) -> Vec<CaptureSkip> {
        self.capture_facts
            .lock()
            .unwrap()
            .iter()
            .filter_map(|fact| match fact {
                CaptureSessionFact::Skip(skip) => Some(skip.clone()),
                CaptureSessionFact::Start(_) | CaptureSessionFact::Fault(_) => None,
            })
            .collect()
    }

    fn validate_capture_identity(&self, identity: &CaptureIdentity) -> Result<(), String> {
        if !identity.is_exact() {
            return Err("capture identity is incomplete".to_string());
        }
        if identity.session_id != self.id {
            return Err("capture fact session does not match live session".to_string());
        }
        let lineage = self.live_lineage.lock().unwrap();
        let Some(lineage) = lineage.as_ref() else {
            return Err("capture fact has no live lineage".to_string());
        };
        if identity.source_id != lineage.source_id
            || identity.build_id != lineage.build_id
            || identity.revision != lineage.revision
        {
            return Err("capture fact does not match live lineage".to_string());
        }
        Ok(())
    }

    /// Record the typed value decisions from the canonical source transaction.
    /// This is a projection only; it never re-runs compatibility checks.
    pub fn record_live_transaction(
        &self,
        snapshot: &SessionSnapshot,
        decisions: &[PersistOutcome],
    ) -> Result<(), String> {
        let before = self.accepted_revision.lock().unwrap().clone();
        self.set_live_lineage(
            &snapshot.source_id,
            &snapshot.build_id,
            &snapshot.revision,
            &snapshot.world_id,
        )?;
        let values = decisions
            .iter()
            .map(persist_outcome_fact)
            .collect::<Vec<_>>();
        *self.live_values.lock().unwrap() = values.clone();
        let lineage = self
            .live_lineage
            .lock()
            .unwrap()
            .clone()
            .expect("validated live lineage");
        *self.accepted_revision.lock().unwrap() = snapshot.revision.clone();
        self.push_receipt(Receipt {
            kind: "live_swap".to_string(),
            status: "committed".to_string(),
            before,
            after: snapshot.revision.clone(),
            client: String::new(),
            output: String::new(),
            live: Some(LiveReceipt { lineage, values }),
        });
        Ok(())
    }
    pub fn record_hot_replace(
        &self,
        snapshot: &SessionSnapshot,
        transaction: &HotReplaceTxn,
    ) -> Result<(), String> {
        self.record_live_transaction(snapshot, transaction.decisions())
    }

    pub fn mark_building(&self) {
        *self.state.lock().unwrap() = "building".to_string();
        self.publish_session_state("building", "", "");
    }

    pub fn mark_ready(&self) {
        *self.state.lock().unwrap() = "ready".to_string();
        self.diagnostic_code.lock().unwrap().clear();
        self.diagnostic.lock().unwrap().clear();
        self.diagnostic_revision.lock().unwrap().clear();
        self.publish_session_state("ready", "", "");
    }
    pub fn mark_error(&self, code: &str, diagnostic: &str) {
        *self.state.lock().unwrap() = "error".to_string();
        *self.diagnostic_code.lock().unwrap() = code.to_string();
        *self.diagnostic.lock().unwrap() = diagnostic.to_string();
        *self.diagnostic_revision.lock().unwrap() =
            self.current_revision.lock().unwrap().clone();
        self.publish_session_state("error", code, diagnostic);
    }
    pub fn mark_stopped(&self) {
        *self.state.lock().unwrap() = "stopped".to_string();
        self.publish_session_state("stopped", "", "");
    }


    /// Return the one canonical devtools stream for this resident session.
    ///
    /// The wire shape deliberately stays the Prelude shape. Host-specific
    /// projections belong outside this adapter; reconnecting hosts receive
    /// the same bounded events and current selection/cursor.
    pub fn devtools_json(&self) -> String {
        self.devtools_lock()
            .envelope
            .serialize()
            .expect("resident devtools envelope must remain serializable")
    }

    pub fn devtools_panel_catalog(
        &self,
    ) -> Result<JetDevtoolsPanelCatalog, JetDevtoolsPanelCatalogError> {
        JetDevtoolsPanelCatalog::canonical()
    }

    pub fn devtools_panels(
        &self,
        host: JetDevtoolsHostKind,
        grants: &[JetDevtoolsPanelCapability],
    ) -> Result<Vec<JetDevtoolsPanelProjection>, JetDevtoolsPanelCatalogError> {
        let catalog = self.devtools_panel_catalog()?;
        let state = self.devtools_lock();
        Ok(catalog.project(&state.envelope, host, grants))
    }



    pub fn current_revision(&self) -> String {
        self.current_revision.lock().unwrap().clone()
    }

    pub fn devtools_command_receipts(&self) -> Vec<JetDevtoolsCommandReceipt> {
        jet_foundation::DevtoolsControl::jet_devtools_command_receipts(&self.id)
    }

    pub fn devtools_command_capabilities(&self) -> DevtoolsCommandCapabilities {
        let reason = if !self.project_rebuild_executor_available() {
            Some("no rebuild executor is registered for this host".to_string())
        } else if self.current_revision().is_empty() {
            Some("source revision is unavailable".to_string())
        } else {
            None
        };
        DevtoolsCommandCapabilities {
            project_rebuild: reason.is_none(),
            project_rebuild_reason: reason,
        }
    }

    pub fn register_project_rebuild_executor(
        &self,
    ) -> Result<ProjectRebuildExecutorGuard, String> {
        let mut available = self
            .project_rebuild_executor
            .lock()
            .map_err(|_| "project rebuild executor state is poisoned".to_string())?;
        if *available {
            return Err("a project rebuild executor is already registered".to_string());
        }
        *available = true;
        drop(available);
        Ok(ProjectRebuildExecutorGuard {
            available: Arc::clone(&self.project_rebuild_executor),
        })
    }

    pub fn project_rebuild_executor_available(&self) -> bool {
        self.project_rebuild_executor
            .lock()
            .map(|available| *available)
            .unwrap_or(false)
    }

    fn validate_devtools_command_admission(
        &self,
        command: &JetDevtoolsCommand,
    ) -> Result<(), String> {
        command.validate()?;
        if command.session_id() != self.id {
            return Err("jet.devtools.v1 command session identity mismatch".to_string());
        }
        if let JetDevtoolsCommand::ProjectRebuild(request) = command {
            if !self.project_rebuild_executor_available() {
                return Err("no rebuild executor is registered for this host".to_string());
            }
            if request.expected_revision != self.current_revision() {
                return Err("source changed since this rebuild was requested".to_string());
            }
        }
        Ok(())
    }

    pub fn enqueue_devtools_command(&self, command: JetDevtoolsCommand) -> Result<(), String> {
        self.validate_devtools_command_admission(&command)?;
        jet_foundation::DevtoolsControl::jet_devtools_enqueue_command(command)
    }

    pub fn enqueue_project_rebuild(
        &self,
        request: JetDevtoolsProjectRebuildRequest,
    ) -> Result<(), String> {
        self.enqueue_devtools_command(JetDevtoolsCommand::ProjectRebuild(request))
    }

    pub fn take_project_rebuild(
        &self,
    ) -> Result<Option<JetDevtoolsProjectRebuildRequest>, String> {
        let Some(command) = self.poll_devtools_command() else {
            return Ok(None);
        };
        let JetDevtoolsCommand::ProjectRebuild(request) = command else {
            jet_foundation::DevtoolsControl::jet_devtools_requeue_command_front(command)?;
            return Ok(None);
        };
        if request.session_id != self.id {
            self.update_command_receipt(
                &request.request_id,
                "failed",
                Some("jet.devtools.v1 command session identity mismatch".to_string()),
            );
            return Err("jet.devtools.v1 command session identity mismatch".to_string());
        }
        if !self.project_rebuild_executor_available() {
            let error = "no rebuild executor is registered for this host".to_string();
            self.update_command_receipt(&request.request_id, "failed", Some(error.clone()));
            return Err(error);
        }
        if request.expected_revision != self.current_revision() {
            let error = "source changed since this rebuild was requested".to_string();
            self.update_command_receipt(&request.request_id, "failed", Some(error.clone()));
            return Err(error);
        }
        self.update_command_receipt(&request.request_id, "building", None);
        Ok(Some(request))
    }

    pub fn finish_project_rebuild(
        &self,
        request: &JetDevtoolsProjectRebuildRequest,
        result: Result<(), String>,
    ) -> Result<(), String> {
        request.validate()?;
        if request.session_id != self.id {
            return Err("jet.devtools.v1 command session identity mismatch".to_string());
        }
        match result {
            Ok(()) => {
                jet_foundation::DevtoolsControl::jet_devtools_update_command_receipt(
                    &self.id,
                    &request.request_id,
                    "completed",
                    None,
                )
            }
            Err(error) => {
                jet_foundation::DevtoolsControl::jet_devtools_update_command_receipt(
                    &self.id,
                    &request.request_id,
                    "failed",
                    Some(error),
                )
            }
        }
    }

    pub fn publish_devtools_event_typed(&self, event: JetDevtoolsEvent) -> Result<u64, String> {
        self.push_devtools_event(event)
    }

    pub fn enqueue_database_explain(
        &self,
        request: JetDevtoolsDatabaseExplainRequest,
    ) -> Result<(), String> {
        self.enqueue_devtools_command(JetDevtoolsCommand::DatabaseExplain(request))
    }

    pub fn enqueue_game_control(
        &self,
        request: JetDevtoolsGameControlRequest,
    ) -> Result<(), String> {
        self.enqueue_devtools_command(JetDevtoolsCommand::GameControl(request))
    }

    pub fn poll_devtools_command(&self) -> Option<JetDevtoolsCommand> {
        jet_foundation::DevtoolsControl::jet_devtools_poll_command(&self.id)
    }

    pub fn poll_database_explain(&self) -> Option<JetDevtoolsDatabaseExplainRequest> {
        jet_foundation::DevtoolsControl::jet_devtools_poll_database_explain(&self.id)
    }

    fn update_command_receipt(
        &self,
        request_id: &str,
        status: &str,
        error: Option<String>,
    ) {
        let _ = jet_foundation::DevtoolsControl::jet_devtools_update_command_receipt(
            &self.id,
            request_id,
            status,
            error,
        );
    }
    /// Dispatch a bounded FIFO slice. Database and game commands share the
    /// Foundation queue; a family-specific destination cannot consume or
    /// reorder the other family.
    pub fn dispatch_pending_devtools_commands(&self) -> Result<usize, String> {
        const MAX_COMMANDS_PER_TICK: usize = 32;
        let pending = jet_foundation::DevtoolsControl::jet_devtools_command_count(&self.id)
            .min(MAX_COMMANDS_PER_TICK);
        let mut dispatched = 0usize;
        for _ in 0..pending {
            let Some(command) = self.poll_devtools_command() else {
                break;
            };
            match command {
                JetDevtoolsCommand::DatabaseExplain(request) => {
                    if request.session_id != self.id {
                        jet_foundation::DevtoolsControl::jet_devtools_requeue_command_front(
                            JetDevtoolsCommand::DatabaseExplain(request),
                        )?;
                        return Err(
                            "jet.devtools.v1 command session identity mismatch".to_string()
                        );
                    }
                    match jet_foundation::Devtools::jet_devtools_dispatch_database_explain(
                        &request.session_id,
                        &request.request_id,
                        &request.statement_identity,
                        request.timeout_ms,
                        request.max_rows,
                    ) {
                        Ok(()) => {
                            self.update_command_receipt(&request.request_id, "dispatched", None);
                            dispatched = dispatched.saturating_add(1);
                        }
                        Err(error)
                            if error == "database EXPLAIN runtime callback is unavailable" =>
                        {
                            self.update_command_receipt(
                                &request.request_id,
                                "waiting",
                                Some(error.clone()),
                            );
                            jet_foundation::DevtoolsControl::jet_devtools_requeue_command_front(
                                JetDevtoolsCommand::DatabaseExplain(request),
                            )?;
                            return Ok(dispatched);
                        }
                        Err(error) => {
                            self.update_command_receipt(
                                &request.request_id,
                                "rejected",
                                Some(error.clone()),
                            );
                            let result = JetDevtoolsDatabaseExplainResult::new(
                                request.session_id.clone(),
                                request.request_id.clone(),
                                request.statement_identity.clone(),
                                "error",
                                0,
                                false,
                                0,
                                Vec::new(),
                                Some("dispatch_failed".to_string()),
                                Some(error),
                            )
                            .and_then(|result| {
                                self.publish_database_explain_result(
                                    result,
                                    unix_time_ms(),
                                    "devserver",
                                )
                            });
                            if let Err(publish_error) = result {
                                jet_foundation::DevtoolsControl::jet_devtools_requeue_command_front(
                                    JetDevtoolsCommand::DatabaseExplain(request),
                                )?;
                                return Err(publish_error);
                            }
                            dispatched = dispatched.saturating_add(1);
                        }
                    }
                }
                JetDevtoolsCommand::GameControl(request) => {
                    if request.session_id != self.id {
                        jet_foundation::DevtoolsControl::jet_devtools_requeue_command_front(
                            JetDevtoolsCommand::GameControl(request),
                        )?;
                        return Err(
                            "jet.devtools.v1 game control session identity mismatch".to_string()
                        );
                    }
                    let payload = request.render_json();
                    match jet_foundation::Devtools::jet_devtools_dispatch_game_control(
                        &request.session_id,
                        &payload,
                    ) {
                        Ok(()) => {
                            self.update_command_receipt(&request.request_id, "dispatched", None);
                            dispatched = dispatched.saturating_add(1);
                        }
                        Err(error)
                            if error == "game control runtime callback is unavailable" =>
                        {
                            match jet_foundation::Devtools::jet_devtools_write_game_control_relay(
                                &request.session_id,
                                &payload,
                            ) {
                                Ok(()) => {
                                    self.update_command_receipt(
                                        &request.request_id,
                                        "relayed",
                                        Some(error.clone()),
                                    );
                                    dispatched = dispatched.saturating_add(1);
                                }
                                Err(relay_error) => {
                                    self.update_command_receipt(
                                        &request.request_id,
                                        "waiting",
                                        Some(relay_error.clone()),
                                    );
                                    jet_foundation::DevtoolsControl::jet_devtools_requeue_command_front(
                                        JetDevtoolsCommand::GameControl(request),
                                    )?;
                                    return Err(relay_error);
                                }
                            }
                        }
                        Err(error) => {
                            self.update_command_receipt(
                                &request.request_id,
                                "rejected",
                                Some(error.clone()),
                            );
                            jet_foundation::DevtoolsControl::jet_devtools_requeue_command_front(
                                JetDevtoolsCommand::GameControl(request),
                            )?;
                            return Err(error);
                        }
                    }
                }
                JetDevtoolsCommand::ProjectRebuild(request) => {
                    jet_foundation::DevtoolsControl::jet_devtools_requeue_command_front(
                        JetDevtoolsCommand::ProjectRebuild(request),
                    )?;
                    return Ok(dispatched);
                }
            }
        }
        Ok(dispatched)
    }

    /// Compatibility name for existing database-only callers.
    pub fn dispatch_pending_database_explain(&self) -> Result<usize, String> {
        self.dispatch_pending_devtools_commands()
    }

    /// Publish a checked database EXPLAIN result as an ordinary typed event.
    /// The event stream remains the only result transport; no result queue is
    /// created beside the command queue.
    pub fn publish_database_explain_result(
        &self,
        result: JetDevtoolsDatabaseExplainResult,
        timestamp_ms: u64,
        source: impl Into<String>,
    ) -> Result<u64, String> {
        if result.session_id != self.id {
            return Err("jet.devtools.v1 EXPLAIN result session identity mismatch".to_string());
        }
        result.validate()?;
        let event = JetDevtoolsEvent::from_parts(
            timestamp_ms,
            source,
            "DatabaseExplainResult",
            result.statement_identity.clone(),
            result.render_json(),
        )?;
        self.publish_devtools_event_typed(event)
    }

    pub fn ingest_devtools_envelope(
        &self,
        decoded: JetDevtoolsDecodedEnvelope,
    ) -> Result<usize, String> {
        decoded.validate()?;
        if decoded.session_id() != self.id {
            return Err("jet.devtools.v1 envelope session identity mismatch".to_string());
        }
        match decoded {
            JetDevtoolsDecodedEnvelope::Events(envelope) => {
                if let Some(identity) = envelope.source_identity.as_ref() {
                    self.set_devtools_source_identity(
                        identity.source_id.clone(),
                        identity.build_id.clone(),
                        identity.revision.clone(),
                        identity.world_id.clone(),
                    )?;
                }
                let events = envelope.events().cloned().collect::<Vec<_>>();
                let count = self.publish_panel_events(events)?;
                let mut state = self.devtools_lock();
                state.envelope.set_selection(envelope.selection().cloned());
                state.envelope.set_cursor(envelope.cursor());
                state.envelope.set_lifecycle(envelope.lifecycle);
                state.envelope.set_freshness(envelope.freshness);
                state.envelope.privacy.loopback_only = envelope.privacy.loopback_only;
                state.envelope.privacy.payload_free_by_default =
                    envelope.privacy.payload_free_by_default;
                state.envelope.privacy.published_payloads |= envelope.privacy.published_payloads;
                state.envelope.reset |= envelope.reset;
                state.envelope.truncated |= envelope.truncated;
                Ok(count)
            }
            JetDevtoolsDecodedEnvelope::Commands(envelope) => {
                let commands = envelope.commands().cloned().collect::<Vec<_>>();
                let count = commands.len();
                for command in &commands {
                    self.validate_devtools_command_admission(command)?;
                }
                let game_count = commands
                    .iter()
                    .filter(|command| matches!(command, JetDevtoolsCommand::GameControl(_)))
                    .count();
                let common_count = count.saturating_sub(game_count);
                let queued_common =
                    jet_foundation::DevtoolsControl::jet_devtools_command_count(&self.id);
                if queued_common > JET_DEVTOOLS_MAX_COMMANDS.saturating_sub(common_count) {
                    return Err("jet.devtools.v1 command queue is full".to_string());
                }
                let mut browser_controls = self.browser_game_controls.lock().unwrap();
                if browser_controls.len() > JET_DEVTOOLS_MAX_COMMANDS.saturating_sub(game_count) {
                    return Err("jet.devtools.v1 browser game command queue is full".to_string());
                }
                for command in commands {
                    match command {
                        JetDevtoolsCommand::GameControl(request) => {
                            let game_command = JetDevtoolsCommand::GameControl(request.clone());
                            browser_controls.push_back(request);
                            jet_foundation::DevtoolsControl::jet_devtools_record_command_receipt(
                                JetDevtoolsCommandReceipt::for_command(
                                    &game_command,
                                    "queued",
                                    None,
                                ),
                            )?;
                        }
                        command => {
                            jet_foundation::DevtoolsControl::jet_devtools_enqueue_command(command)?;
                        }
                    }
                }
                Ok(count)
            }
        }
    }
    pub fn publish_panel_events<I>(&self, events: I) -> Result<usize, String>
    where
        I: IntoIterator<Item = JetDevtoolsEvent>,
    {
        let mut count = 0usize;
        for event in events {
            self.publish_devtools_event_typed(event)?;
            count = count.saturating_add(1);
        }
        Ok(count)
    }

    pub fn publish_job_fact(
        &self,
        fact: &JetDevtoolsJobPanelFact,
        source: impl Into<String>,
    ) -> Result<u64, String> {
        let event = fact.to_protocol_event(source)?;
        self.publish_devtools_event_typed(event)
    }

    pub fn publish_database_panel(
        &self,
        facts: &JetDevtoolsDatabasePanelFacts,
        source: impl Into<String>,
    ) -> Result<usize, String> {
        let events = facts
            .to_protocol_events(source)
            .map_err(|error| error.to_string())?;
        self.publish_panel_events(events)
    }
    pub fn publish_request_fact(
        &self,
        fact: &JetDevtoolsRequestPanelFact,
        source: impl Into<String>,
    ) -> Result<u64, String> {
        self.publish_devtools_event_typed(fact.to_protocol_event(source)?)
    }

    pub fn publish_request_facts<I>(
        &self,
        facts: I,
        source: impl Into<String>,
    ) -> Result<usize, String>
    where
        I: IntoIterator<Item = JetDevtoolsRequestPanelFact>,
    {
        let source = source.into();
        let events = facts
            .into_iter()
            .map(|fact| fact.to_protocol_event(source.clone()))
            .collect::<Result<Vec<_>, _>>()?;
        self.publish_panel_events(events)
    }

    pub fn publish_request_panel(
        &self,
        panel: &JetDevtoolsRequestPanelState,
        source: impl Into<String>,
    ) -> Result<usize, String> {
        self.publish_panel_events(panel.to_protocol_events(source)?)
    }

    pub fn publish_telemetry_panel(
        &self,
        panel: &JetDevtoolsTelemetryPanelState,
        source: impl Into<String>,
    ) -> Result<usize, String> {
        self.publish_panel_events(panel.to_protocol_events(source)?)
    }

    pub fn publish_topology_fact(
        &self,
        fact: &JetDevtoolsTopologyFact,
        source: impl Into<String>,
    ) -> Result<u64, String> {
        self.publish_devtools_event_typed(fact.to_protocol_event(source)?)
    }

    pub fn publish_topology_panel(
        &self,
        state: &JetDevtoolsTopologyState,
        source: impl Into<String>,
    ) -> Result<usize, String> {
        self.publish_panel_events(state.to_protocol_events(source)?)
    }



    /// Move the resident replay cursor to one retained sequence.
    pub fn scrub(&self, sequence: u64) -> Result<ReplayProjection, String> {
        let projection = {
            let mut recording = self.recording.lock().map_err(|_| {
                "devtools recording state is unavailable because its lock is poisoned".to_string()
            })?;
            let state = recording
                .as_mut()
                .ok_or_else(recording_unavailable)?;
            state
                .cursor
                .seek(&state.recording, sequence)
                .map_err(recording_error)
        };
        if let Ok(projection) = projection.as_ref() {
            self.sync_replay_cursor(projection);
        }
        projection
    }

    /// Move the resident replay cursor to the newest event at or before time.
    pub fn scrub_time(&self, timestamp_ms: u64) -> Result<ReplayProjection, String> {
        let projection = {
            let mut recording = self.recording.lock().map_err(|_| {
                "devtools recording state is unavailable because its lock is poisoned".to_string()
            })?;
            let state = recording
                .as_mut()
                .ok_or_else(recording_unavailable)?;
            state
                .cursor
                .seek_time(&state.recording, timestamp_ms)
                .map_err(recording_error)
        };
        if let Ok(projection) = projection.as_ref() {
            self.sync_replay_cursor(projection);
        }
        projection
    }

    /// Advance the resident replay cursor by one event.
    pub fn step(&self) -> Result<ReplayProjection, String> {
        let projection = {
            let mut recording = self.recording.lock().map_err(|_| {
                "devtools recording state is unavailable because its lock is poisoned".to_string()
            })?;
            let state = recording
                .as_mut()
                .ok_or_else(recording_unavailable)?;
            state
                .cursor
                .step_forward(&state.recording)
                .map_err(recording_error)
        };
        if let Ok(projection) = projection.as_ref() {
            self.sync_replay_cursor(projection);
        }
        projection
    }

    /// Advance the resident replay cursor by an explicit bounded count.
    pub fn skip(&self, count: u64) -> Result<ReplayProjection, String> {
        let projection = {
            let mut recording = self.recording.lock().map_err(|_| {
                "devtools recording state is unavailable because its lock is poisoned".to_string()
            })?;
            let state = recording
                .as_mut()
                .ok_or_else(recording_unavailable)?;
            for _ in 0..count {
                state
                    .cursor
                    .step_forward(&state.recording)
                    .map_err(recording_error)?;
            }
            state
                .recording
                .projection(&state.cursor)
                .map_err(recording_error)
        };
        if let Ok(projection) = projection.as_ref() {
            self.sync_replay_cursor(projection);
        }
        projection
    }

    /// Save a named, identity- and generation-bound replay checkpoint.
    pub fn checkpoint(&self, name: &str) -> Result<SessionCheckpoint, String> {
        validate_devtools_text(name, "checkpoint name")?;
        let mut recording = self.recording.lock().map_err(|_| {
            "devtools recording state is unavailable because its lock is poisoned".to_string()
        })?;
        let state = recording
            .as_mut()
            .ok_or_else(recording_unavailable)?;
        let checkpoint = state
            .recording
            .checkpoint(&state.cursor)
            .map_err(recording_error)?;
        state
            .checkpoints
            .insert(name.to_string(), checkpoint.clone());
        Ok(checkpoint)
    }

    /// Restore a named checkpoint and return its canonical projection.
    pub fn restore(&self, name: &str) -> Result<ReplayProjection, String> {
        validate_devtools_text(name, "checkpoint name")?;
        let projection = {
            let mut recording = self.recording.lock().map_err(|_| {
                "devtools recording state is unavailable because its lock is poisoned".to_string()
            })?;
            let state = recording
                .as_mut()
                .ok_or_else(recording_unavailable)?;
            let checkpoint = state
                .checkpoints
                .get(name)
                .cloned()
                .ok_or_else(|| RecordingError::CheckpointUnavailable.to_string())?;
            state.cursor = state
                .recording
                .restore_checkpoint(&checkpoint)
                .map_err(recording_error)?;
            state
                .recording
                .projection(&state.cursor)
                .map_err(recording_error)
        };
        if let Ok(projection) = projection.as_ref() {
            self.sync_replay_cursor(projection);
        }
        projection
    }

    /// Compare the retained recording with the canonical resident event tail.
    pub fn divergence(&self) -> Result<DevtoolsRecordingDivergence, String> {
        let (envelope_sequence, envelope_identity) = {
            let state = self.devtools_lock();
            (
                state.envelope.latest_sequence(),
                state.envelope.source_identity.clone(),
            )
        };
        let recording = self.recording.lock().map_err(|_| {
            "devtools recording state is unavailable because its lock is poisoned".to_string()
        })?;
        let state = recording
            .as_ref()
            .ok_or_else(recording_unavailable)?;
        let expected_sequence = state.first_sequence.and_then(|first| {
            envelope_sequence.filter(|sequence| *sequence >= first)
        });
        let actual_sequence = state.recording.events().last().map(SessionEvent::sequence);
        let mut reason = None;
        if envelope_identity.as_ref().map_or(true, |identity| {
            identity.source_id.as_deref() != Some(state.recording.identity().source_id.as_str())
                || identity.build_id.as_deref() != Some(state.recording.identity().build_id.as_str())
        }) {
            reason = Some("resident source/build identity differs from recording".to_string());
        } else if expected_sequence != actual_sequence {
            reason = Some("resident event tail differs from recording".to_string());
        } else if let Err(error) = state.recording.projection(&state.cursor) {
            reason = Some(error.to_string());
        }
        let truncation = state.recording.truncation();
        Ok(DevtoolsRecordingDivergence {
            divergent: reason.is_some(),
            reason,
            identity: state.recording.identity().clone(),
            expected_sequence,
            actual_sequence,
            retained_events: state.recording.event_count(),
            dropped_events: truncation.dropped_events,
            truncation,
        })
    }

    /// Apply one typed replay operation from the HTTP boundary.
    pub fn recording_control(&self, payload: &str) -> Result<String, String> {
        let root = parse_json_with_limit(payload, JET_DEVTOOLS_MAX_ENVELOPE_BYTES)
            .map_err(|_| "recording control must be valid bounded JSON".to_string())?;
        let object = object_map(&root)
            .ok_or_else(|| "recording control must be a JSON object".to_string())?;
        let operation = object
            .get("op")
            .or_else(|| object.get("action"))
            .and_then(json_string)
            .ok_or_else(|| "recording control must include string op".to_string())?;
        match operation {
            "scrub" => {
                let sequence = object
                    .get("sequence")
                    .and_then(json_u64)
                    .ok_or_else(|| "recording scrub requires non-negative sequence".to_string())?;
                self.scrub(sequence)?;
            }
            "scrub_time" => {
                let timestamp_ms = object
                    .get("timestamp_ms")
                    .and_then(json_u64)
                    .ok_or_else(|| {
                        "recording scrub_time requires non-negative timestamp_ms".to_string()
                    })?;
                self.scrub_time(timestamp_ms)?;
            }
            "step" => match object.get("direction").and_then(json_string) {
                None | Some("forward") => {
                    self.step()?;
                }
                Some("backward") | Some("reverse") => {
                    return Err("recording step is forward-only".to_string());
                }
                Some(direction) => {
                    return Err(format!("unknown recording step direction `{direction}`"));
                }
            },
            "skip" => {
                let count = object
                    .get("count")
                    .and_then(json_u64)
                    .ok_or_else(|| "recording skip requires non-negative count".to_string())?;
                self.skip(count)?;
            }
            "checkpoint" => {
                let name = object
                    .get("name")
                    .and_then(json_string)
                    .ok_or_else(|| "recording checkpoint requires string name".to_string())?;
                self.checkpoint(name)?;
            }
            "restore" => {
                let name = object
                    .get("name")
                    .and_then(json_string)
                    .ok_or_else(|| "recording restore requires string name".to_string())?;
                self.restore(name)?;
            }
            "divergence" => {
                self.divergence()?;
            }
            _ => return Err(format!("unknown recording operation `{operation}`")),
        }
        Ok(self.recording_json())
    }

    /// Serialize the canonical resident recording and current replay cursor.
    pub fn recording_json(&self) -> String {
        let divergence = self.divergence().ok();
        let state = match self.recording.lock() {
            Ok(state) => state,
            Err(_) => {
                return recording_unavailable_json(
                    &self.id,
                    "devtools recording state is unavailable because its lock is poisoned",
                )
            }
        };
        let Some(state) = state.as_ref() else {
            return recording_unavailable_json(
                &self.id,
                "source/build identity is unavailable",
            );
        };
        let projection = match state.recording.projection(&state.cursor) {
            Ok(projection) => projection,
            Err(error) => return recording_unavailable_json(&self.id, &error.to_string()),
        };
        recording_json_for_state(&self.id, state, &projection, divergence.as_ref())
    }

    pub fn set_devtools_source_identity(
        &self,
        source_id: Option<String>,
        build_id: Option<String>,
        revision: Option<String>,
        world_id: Option<String>,
    ) -> Result<(), String> {
        let recording_identity = match (source_id.as_deref(), build_id.as_deref()) {
            (Some(source_id), Some(build_id)) => Some(
                SessionIdentity::new(source_id, build_id).map_err(recording_error)?,
            ),
            _ => None,
        };
        let identity =
            JetDevtoolsSourceIdentityFact::new(source_id, build_id, revision, world_id);
        let first_sequence = {
            let mut state = self.devtools_lock();
            state.envelope.set_source_identity(Some(identity))?;
            match state.envelope.latest_sequence() {
                Some(sequence) => Some(
                    sequence
                        .checked_add(1)
                        .ok_or_else(|| "devtools event sequence exhausted".to_string())?,
                ),
                None => Some(1),
            }
        };
        self.set_recording_identity(recording_identity, first_sequence)
    }

    fn set_recording_identity(
        &self,
        identity: Option<SessionIdentity>,
        first_sequence: Option<u64>,
    ) -> Result<(), String> {
        let mut recording = self.recording.lock().map_err(|_| {
            "devtools recording state is unavailable because its lock is poisoned".to_string()
        })?;
        if let (Some(state), Some(identity)) = (recording.as_ref(), identity.as_ref()) {
            if state.recording.identity() == identity {
                return Ok(());
            }
        }
        *recording = match identity {
            Some(identity) => Some(ResidentRecordingState::new(
                SessionRecording::new(identity).map_err(recording_error)?,
                first_sequence,
            )),
            None => None,
        };
        Ok(())
    }

    pub fn set_devtools_lifecycle(
        &self,
        lifecycle: JetDevtoolsLifecycleState,
        freshness: JetDevtoolsFreshnessFact,
    ) {
        let mut state = self.devtools_lock();
        state.envelope.set_lifecycle(lifecycle);
        state.envelope.set_freshness(freshness);
    }



    /// Project the sema-owned HotSwap verdict into the shared devtools
    /// stream.  Compatibility and retention are facts here; this adapter
    /// never recomputes them from source or runtime state.
    pub fn publish_hot_swap_decision(
        &self,
        decision: &HotSwapDecision,
    ) -> Result<(), String> {
        validate_devtools_text(&decision.module, "HotSwap module")?;
        let compatibility = if decision.is_compatible() {
            "compatible"
        } else {
            "incompatible"
        };
        let reason = decision.compatibility.reason().unwrap_or("");
        let (kept, reset): (Vec<_>, Vec<_>) = decision
            .state
            .iter()
            .partition(|fact| fact.is_preserved());
        let state = decision
            .state
            .iter()
            .map(retention_fact_json)
            .collect::<Vec<_>>()
            .join(",");
        let change_facts = decision
            .change_facts
            .iter()
            .map(game_change_fact_json)
            .collect::<Vec<_>>()
            .join(",");
        let fields = format!(
            "{{\"module\":{},\"compatibility\":{},\"reason\":{},\"changed\":{},\"changed_functions\":{},\"state\":[{}],\"kept\":{},\"reset\":{},\"rechecked_items\":{},\"change_facts\":[{}]}}",
            json_value(&decision.module),
            json_value(compatibility),
            json_value(reason),
            string_array_json(&decision.changed),
            string_array_json(&decision.changed_functions),
            state,
            string_array_json(
                &kept
                    .iter()
                    .map(|fact| fact.key.clone())
                    .collect::<Vec<_>>(),
            ),
            string_array_json(
                &reset
                    .iter()
                    .map(|fact| fact.key.clone())
                    .collect::<Vec<_>>(),
            ),
            string_array_json(&decision.rechecked_items),
            change_facts,
        );
        let event = JetDevtoolsEvent::from_parts(
            unix_time_ms(),
            "sema",
            "HotSwap",
            "module",
            fields,
        )?;
        self.publish_devtools_event_typed(event).map(|_| ())
    }

    /// Last input wins for selection.  The update also feeds the existing
    /// Canvas selection fields so older Canvas clients and new hosts agree.
    pub fn set_devtools_selection(&self, request: &str) -> Result<(), String> {
        let root = parse_json(request)
            .map_err(|_| "devtools selection must be valid JSON".to_string())?;
        let object =
            object_map(&root).ok_or_else(|| "devtools selection must be an object".to_string())?;
        let nested = object.get("selection").and_then(object_map);
        let field = |key: &str| {
            object
                .get(key)
                .or_else(|| nested.as_ref().and_then(|selection| selection.get(key)))
                .and_then(json_string)
                .map(str::to_string)
        };
        let clear = object
            .get("clear")
            .is_some_and(|value| matches!(value, DataTree::Bool(true)))
            || matches!(object.get("selection"), Some(DataTree::Null));
        let panel_id = field("panel_id").or_else(|| field("target"));
        let item_key = field("item_key")
            .or_else(|| field("source_id"))
            .or_else(|| field("output"));
        let origin = field("origin").unwrap_or_else(|| "host".to_string());
        validate_devtools_text(&origin, "selection origin")?;
        for (value, label) in [
            (panel_id.as_deref(), "panel_id"),
            (item_key.as_deref(), "item_key"),
        ] {
            if let Some(value) = value {
                validate_devtools_text(value, label)?;
            }
        }
        if !clear && panel_id.is_none() && item_key.is_none() {
            return Err("devtools selection has no selected value".to_string());
        }
        let source_id = field("source_id");
        let output = field("output");
        let target = field("target");
        if clear {
            *self.selected_source_id.lock().unwrap() = String::new();
            *self.selected_output.lock().unwrap() = String::new();
            *self.selected_target.lock().unwrap() = String::new();
        } else {
            if let Some(source_id) = source_id.as_deref().or(item_key.as_deref()) {
                self.select_project_source(source_id);
            }
            self.select_output_values(
                output.as_deref(),
                target.as_deref().or(panel_id.as_deref()),
            );
        }
        {
            let mut state = self.devtools_lock();
            if clear {
                state.envelope.set_selection(None);
            } else {
                let previous_panel = state
                    .envelope
                    .view()
                    .selection()
                    .map(|selection| selection.panel_id.clone());
                let selected_panel = panel_id
                    .clone()
                    .or(previous_panel)
                    .unwrap_or_default();
                let selected_item = item_key.clone().or_else(|| {
                    state
                        .envelope
                        .view()
                        .selection()
                        .and_then(|selection| selection.item_key.clone())
                });
                state.envelope.set_selection(Some(JetDevtoolsSelection::new(
                    selected_panel,
                    selected_item,
                )));
            }
        }
        let fields = format!(
            "{{\"panel_id\":{},\"item_key\":{},\"cleared\":{}}}",
            if clear {
                "null".to_string()
            } else {
                panel_id
                    .as_deref()
                    .map(json_value)
                    .unwrap_or_else(|| "null".to_string())
            },
            if clear {
                "null".to_string()
            } else {
                item_key
                    .as_deref()
                    .map(json_value)
                    .unwrap_or_else(|| "null".to_string())
            },
            clear,
        );
        let event = JetDevtoolsEvent::from_parts(
            unix_time_ms(),
            origin,
            "Selection",
            "session",
            fields,
        )?;
        self.publish_devtools_event_typed(event).map(|_| ())
    }

    /// Update the shared time cursor.  The canonical cursor is an event
    /// sequence, so all hosts can resolve it against the bounded history.
    pub fn set_devtools_cursor(&self, request: &str) -> Result<(), String> {
        let root = parse_json(request)
            .map_err(|_| "devtools cursor must be valid JSON".to_string())?;
        let object =
            object_map(&root).ok_or_else(|| "devtools cursor must be an object".to_string())?;
        let origin = object
            .get("origin")
            .and_then(json_string)
            .unwrap_or("host");
        validate_devtools_text(origin, "cursor origin")?;
        let cursor_value = object
            .get("cursor")
            .or_else(|| object.get("sequence"));
        let sequence = match cursor_value {
            None | Some(DataTree::Null) => None,
            Some(DataTree::Object(cursor)) => {
                let cursor = cursor.iter().cloned().collect::<BTreeMap<_, _>>();
                match cursor.get("sequence") {
                    Some(value) => Some(
                        json_u64(value)
                            .ok_or_else(|| "devtools cursor must be a sequence".to_string())?,
                    ),
                    None => {
                        return Err("devtools cursor must be a sequence".to_string());
                    }
                }
            }
            Some(value) => Some(
                json_u64(value)
                    .ok_or_else(|| "devtools cursor must be a sequence".to_string())?,
            ),
        };
        {
            self.devtools_lock().envelope.set_cursor(sequence);
        }
        let fields = format!(
            "{{\"sequence\":{}}}",
            sequence
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_string()),
        );
        let event = JetDevtoolsEvent::from_parts(
            unix_time_ms(),
            origin,
            "Cursor",
            "session",
            fields,
        )?;
        self.publish_devtools_event_typed(event).map(|_| ())
    }

    fn publish_session_state(&self, state_name: &str, code: &str, diagnostic: &str) {
        let lifecycle = devtools_session_state(state_name);
        let freshness = if lifecycle == DevtoolsSessionState::Ready {
            JetDevtoolsFreshnessFact::fresh(unix_time_ms())
        } else {
            JetDevtoolsFreshnessFact::stale(unix_time_ms())
        };
        {
            let mut state = self.devtools_lock();
            state.envelope.set_lifecycle(lifecycle);
            state.envelope.set_freshness(freshness);
        }
        let fields = format!(
            "{{\"state\":{},\"code\":{},\"diagnostic\":{}}}",
            json_value(state_name),
            json_value(code),
            json_value(diagnostic),
        );
        if let Ok(event) = JetDevtoolsEvent::from_parts(
            unix_time_ms(),
            "devserver",
            "state",
            "session",
            fields,
        ) {
            let _ = self.publish_devtools_event_typed(event);
        }
    }
    fn push_devtools_event(&self, event: JetDevtoolsEvent) -> Result<u64, String> {
        let mut devtools = self.devtools_lock();
        let sequence = match devtools.envelope.latest_sequence() {
            Some(sequence) => sequence
                .checked_add(1)
                .ok_or_else(|| "devtools event sequence exhausted".to_string())?,
            None => 1,
        };
        let mut recording = self.recording.lock().map_err(|_| {
            "devtools recording state is unavailable because its lock is poisoned".to_string()
        })?;
        if let Some(state) = recording.as_mut() {
            let recorded = SessionEvent::new(
                sequence,
                event.timestamp_ms,
                event.source().to_string(),
                event.kind().to_string(),
                event.entity().to_string(),
                event.fields_json().to_string(),
                event.payload_json().map(str::to_string),
            )
            .map_err(recording_error)?;
            state
                .recording
                .append_event(recorded)
                .map_err(recording_error)?;
        }
        let assigned = devtools.envelope.push(event);
        if let Some(state) = recording.as_mut() {
            let mut cursor = state.recording.replay();
            for _ in 0..state.recording.event_count() {
                cursor
                    .step_forward(&state.recording)
                    .map_err(recording_error)?;
            }
            state.cursor = cursor;
            state.checkpoints.clear();
        }
        Ok(assigned)
    }

    fn devtools_lock(&self) -> std::sync::MutexGuard<'_, DevtoolsState> {
        match self.devtools.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
    fn sync_replay_cursor(&self, projection: &ReplayProjection) {
        self.devtools_lock()
            .envelope
            .set_cursor(projection.sequence());
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
            live: None,
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
            live: None,
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
            live: None,
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

    /// Return the terminal's current semantic target, if one was selected.
    /// Game controls use this fact rather than manufacturing an actor id.
    pub fn selected_target(&self) -> Option<String> {
        let target = self.selected_target.lock().unwrap().clone();
        (!target.is_empty()).then_some(target)
    }

    /// Return the terminal's current selected output, if one was selected.
    pub fn selected_output(&self) -> Option<String> {
        let output = self.selected_output.lock().unwrap().clone();
        (!output.is_empty()).then_some(output)
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
        let live_lineage = self.live_lineage.lock().unwrap().clone();
        let live_values = self.live_values.lock().unwrap().clone();
        let (live_protocol, live_session_id, live_source_id, live_build_id, live_revision, live_world_id) =
            live_lineage
                .as_ref()
                .map(|lineage| {
                    (
                        json_value(&lineage.protocol),
                        json_value(&lineage.session_id),
                        json_value(&lineage.source_id),
                        json_value(&lineage.build_id),
                        json_value(&lineage.revision),
                        json_value(&lineage.world_id),
                    )
                })
                .unwrap_or_else(|| {
                    (
                        "null".to_string(),
                        "null".to_string(),
                        "null".to_string(),
                        "null".to_string(),
                        "null".to_string(),
                        "null".to_string(),
                    )
                });
        let live_values_json = live_values
            .iter()
            .map(live_value_fact_json)
            .collect::<Vec<_>>()
            .join(",");
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
                let live = receipt
                    .live
                    .as_ref()
                    .map(live_receipt_json)
                    .unwrap_or_else(|| "null".to_string());
                format!(
                    "{{\"kind\":{},\"status\":{},\"before\":{},\"after\":{},\"client\":{},\"output\":{},\"live\":{}}}",
                    json_value(&receipt.kind),
                    json_value(&receipt.status),
                    json_value(&receipt.before),
                    json_value(&receipt.after),
                    json_value(&receipt.client),
                    json_value(&receipt.output),
                    live,
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"id\":{},\"protocol\":{},\"session_id\":{},\"source_id\":{},\"build_id\":{},\"revision\":{},\"world_id\":{},\"values\":[{}],\"entry\":{},\"project_context\":{{\"source_id\":{}}},\"source_revision\":{},\"accepted_revision\":{},\"last_good_revision\":{},\"last_good_program\":{},\"state\":{},\"diagnostic_code\":{},\"diagnostic\":{},\"diagnostic_revision\":{},\"last_good_views\":{{\"graph\":{},\"debugger\":{},\"runtime\":{}}},\"clients\":{},\"run\":{{\"output\":{},\"target\":{}}},\"debugger\":{{\"state\":{},\"session_id\":{},\"source_id\":{},\"revision\":{},\"tier\":{}}},\"tests\":{{\"state\":{}}},\"history\":{{\"count\":{},\"receipts\":[{}]}},\"listeners\":{},\"custom_servers\":{{\"owner\":\"application\",\"transport\":\"application\",\"reload\":\"source-transaction\",\"listener\":{}}}}}",
            json_value(&self.id),
            live_protocol,
            live_session_id,
            live_source_id,
            live_build_id,
            live_revision,
            live_world_id,
            live_values_json,
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

    /// Register this resident session as one scoped canonical event sink.
    /// The returned guard must be retained for the desired lifecycle.
    pub fn install_devtools_event_sink(
        self: &std::sync::Arc<Self>,
    ) -> JetDevtoolsEventSinkGuard {
        let sink: std::sync::Arc<dyn JetDevtoolsEventSink> = self.clone();
        jet_foundation::Devtools::jet_devtools_install_event_sink(sink)
    }
}
impl Drop for ResidentDevSession {
    fn drop(&mut self) {
        jet_foundation::DevtoolsControl::jet_devtools_clear_commands(&self.id);
        let _ = jet_foundation::Devtools::jet_devtools_clear_game_control_relays();
    }
}

impl JetDevtoolsEventSink for ResidentDevSession {
    fn publish(&self, event: JetDevtoolsEvent) {
        let _ = self.publish_devtools_event_typed(event);
    }
}

fn trim_capture_facts(facts: &mut Vec<CaptureSessionFact>) {
    if facts.len() > MAX_RECEIPTS {
        let drop_count = facts.len() - MAX_RECEIPTS;
        facts.drain(..drop_count);
    }
}

impl crate::CapturePolicy::RecordIndexCaptureSink for ResidentDevSession {
    type Error = String;

    fn record_capture_start(&mut self, fact: &CaptureStartFact) -> Result<(), Self::Error> {
        ResidentDevSession::record_capture_start(self, fact)
    }

    fn record_capture_fault(&mut self, fact: &CaptureFaultFact) -> Result<(), Self::Error> {
        ResidentDevSession::record_capture_fault(self, fact)
    }
}

fn persist_outcome_fact(outcome: &PersistOutcome) -> LiveValueFact {
    match outcome {
        PersistOutcome::Kept(entry) => LiveValueFact {
            value_id: entry.stable_key(),
            type_identity: entry.type_identity().to_string(),
            disposition: "kept".to_string(),
            reason: String::new(),
        },
        PersistOutcome::Migrated(entry) => LiveValueFact {
            value_id: entry.stable_key(),
            type_identity: entry.type_identity().to_string(),
            disposition: "migrated".to_string(),
            reason: "published schema migration applied".to_string(),
        },
        PersistOutcome::Reset { reason, entry } => LiveValueFact {
            value_id: entry.stable_key(),
            type_identity: entry.type_identity().to_string(),
            disposition: "reset".to_string(),
            reason: reason.clone(),
        },
        PersistOutcome::Rejected { reason, prior } => {
            let type_identity = match reason {
                PersistRejectReason::MigrationUnavailable { from, .. }
                | PersistRejectReason::ShapeChanged { from, .. } => from.clone(),
                _ => "unavailable".to_string(),
            };
            LiveValueFact {
                value_id: prior.key(),
                type_identity,
                disposition: "rejected".to_string(),
                reason: reason.message(),
            }
        }
    }
}

fn optional_fact(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_string())
}

fn devtools_session_state(value: &str) -> DevtoolsSessionState {
    match value {
        "starting" => DevtoolsSessionState::Starting,
        "building" => DevtoolsSessionState::Building,
        "ready" => DevtoolsSessionState::Ready,
        "error" => DevtoolsSessionState::Error,
        "stopped" => DevtoolsSessionState::Stopped,
        _ => DevtoolsSessionState::Unavailable,
    }
}
fn live_value_fact_json(value: &LiveValueFact) -> String {
    format!(
        "{{\"value_id\":{},\"type_identity\":{},\"disposition\":{},\"reason\":{}}}",
        json_value(&value.value_id),
        json_value(&value.type_identity),
        json_value(&value.disposition),
        json_value(&value.reason),
    )
}

fn live_receipt_json(receipt: &LiveReceipt) -> String {
    let values = receipt
        .values
        .iter()
        .map(live_value_fact_json)
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"protocol\":{},\"session_id\":{},\"source_id\":{},\"build_id\":{},\"revision\":{},\"world_id\":{},\"values\":[{}]}}",
        json_value(&receipt.lineage.protocol),
        json_value(&receipt.lineage.session_id),
        json_value(&receipt.lineage.source_id),
        json_value(&receipt.lineage.build_id),
        json_value(&receipt.lineage.revision),
        json_value(&receipt.lineage.world_id),
        values,
    )
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

fn recording_error(error: RecordingError) -> String {
    error.to_string()
}

fn recording_unavailable() -> String {
    "devtools recording is unavailable because source/build identity is not installed".to_string()
}

fn recording_unavailable_json(session_id: &str, reason: &str) -> String {
    format!(
        "{{\"protocol\":{},\"session_id\":{},\"kind\":\"session.recording\",\"available\":false,\"reason\":{},\"events\":[],\"cursor\":null,\"divergence\":null}}",
        json_value(JET_DEVTOOLS_PROTOCOL),
        json_value(session_id),
        json_value(reason),
    )
}

fn recording_json_for_state(
    session_id: &str,
    state: &ResidentRecordingState,
    projection: &ReplayProjection,
    divergence: Option<&DevtoolsRecordingDivergence>,
) -> String {
    let identity = state.recording.identity();
    let events = state
        .recording
        .events()
        .map(recording_event_json)
        .collect::<Vec<_>>()
        .join(",");
    let checkpoints = {
        let mut values = state.checkpoints.iter().collect::<Vec<_>>();
        values.sort_by(|(left, _), (right, _)| left.cmp(right));
        values
            .into_iter()
            .map(|(name, checkpoint)| recording_checkpoint_json(name, checkpoint))
            .collect::<Vec<_>>()
            .join(",")
    };
    let current_event = projection
        .event()
        .map(recording_event_json)
        .unwrap_or_else(|| "null".to_string());
    let truncation = &state.recording.truncation();
    let divergence = divergence
        .map(recording_divergence_json)
        .unwrap_or_else(|| "null".to_string());
    format!(
        "{{\"protocol\":{},\"session_id\":{},\"kind\":\"session.recording\",\"available\":true,\"identity\":{{\"source_id\":{},\"build_id\":{}}},\"events\":[{}],\"truncation\":{{\"event_limit\":{},\"byte_limit\":{},\"retained_events\":{},\"retained_bytes\":{},\"dropped_events\":{},\"dropped_bytes\":{},\"event_limit_reached\":{},\"byte_limit_reached\":{},\"first_dropped_sequence\":{},\"last_dropped_sequence\":{},\"first_retained_sequence\":{},\"last_retained_sequence\":{}}},\"cursor\":{{\"identity\":{{\"source_id\":{},\"build_id\":{}}},\"generation\":{},\"position\":{},\"sequence\":{},\"timestamp_ms\":{},\"previous_sequence\":{},\"next_sequence\":{},\"event\":{}}},\"checkpoints\":[{}],\"divergence\":{}}}",
        json_value(JET_DEVTOOLS_PROTOCOL),
        json_value(session_id),
        json_value(&identity.source_id),
        json_value(&identity.build_id),
        events,
        truncation.event_limit,
        truncation.byte_limit,
        truncation.retained_events,
        truncation.retained_bytes,
        truncation.dropped_events,
        truncation.dropped_bytes,
        truncation.event_limit_reached,
        truncation.byte_limit_reached,
        optional_u64_json(truncation.first_dropped_sequence),
        optional_u64_json(truncation.last_dropped_sequence),
        optional_u64_json(truncation.first_retained_sequence),
        optional_u64_json(truncation.last_retained_sequence),
        json_value(&projection.identity().source_id),
        json_value(&projection.identity().build_id),
        state.cursor.generation(),
        projection.position(),
        optional_u64_json(projection.sequence()),
        optional_u64_json(projection.timestamp_ms()),
        optional_u64_json(projection.previous_sequence()),
        optional_u64_json(projection.next_sequence()),
        current_event,
        checkpoints,
        divergence,
    )
}

fn recording_event_json(event: &SessionEvent) -> String {
    format!(
        "{{\"sequence\":{},\"timestamp_ms\":{},\"source\":{},\"kind\":{},\"entity\":{},\"fields\":{},\"payload\":{}}}",
        event.sequence(),
        event.timestamp_ms(),
        json_value(event.source()),
        json_value(event.kind()),
        json_value(event.entity()),
        event.fields_json(),
        event.payload_json().unwrap_or("null"),
    )
}

fn recording_checkpoint_json(name: &str, checkpoint: &SessionCheckpoint) -> String {
    format!(
        "{{\"name\":{},\"identity\":{{\"source_id\":{},\"build_id\":{}}},\"generation\":{},\"position\":{},\"sequence\":{},\"timestamp_ms\":{}}}",
        json_value(name),
        json_value(&checkpoint.identity().source_id),
        json_value(&checkpoint.identity().build_id),
        checkpoint.generation(),
        checkpoint.position(),
        optional_u64_json(checkpoint.sequence()),
        optional_u64_json(checkpoint.timestamp_ms()),
    )
}

fn recording_divergence_json(divergence: &DevtoolsRecordingDivergence) -> String {
    format!(
        "{{\"divergent\":{},\"reason\":{},\"expected_sequence\":{},\"actual_sequence\":{},\"retained_events\":{},\"dropped_events\":{}}}",
        divergence.divergent,
        divergence
            .reason
            .as_deref()
            .map(json_value)
            .unwrap_or_else(|| "null".to_string()),
        optional_u64_json(divergence.expected_sequence),
        optional_u64_json(divergence.actual_sequence),
        divergence.retained_events,
        divergence.dropped_events,
    )
}

fn optional_u64_json(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string())
}

fn json_value(value: &str) -> String {
    if value.is_empty() {
        "null".to_string()
    } else {
        format!("\"{}\"", json_escape(value))
    }
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn json_string(value: &DataTree) -> Option<&str> {
    match value {
        DataTree::Text(value) | DataTree::TypedText(value) => Some(value),
        _ => None,
    }
}

fn json_u64(value: &DataTree) -> Option<u64> {
    match value {
        DataTree::Int(value) if *value >= 0 => (*value).try_into().ok(),
        DataTree::Float(value)
            if value.fract() == 0.0 && *value >= 0.0 && *value <= u64::MAX as f64 =>
        {
            Some(*value as u64)
        }
        _ => None,
    }
}

fn validate_devtools_text(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > DEVTOOLS_TEXT_LIMIT || value.chars().any(char::is_control) {
        return Err(format!("devtools {label} is empty, too long, or contains control text"));
    }
    Ok(())
}


fn string_array_json(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| json_value(value))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn retention_fact_json(fact: &jet_foundation::HotSwap::StateRetentionFact) -> String {
    let (decision, reason) = match &fact.decision {
        StateRetentionDecision::Preserve => ("preserve", ""),
        StateRetentionDecision::Fresh { reason } => ("fresh", reason.as_str()),
    };
    format!(
        "{{\"key\":{},\"decision\":{},\"reason\":{}}}",
        json_value(&fact.key),
        json_value(decision),
        json_value(reason),
    )
}

fn game_change_fact_json(fact: &jet_foundation::Game::JetGameChangeFact) -> String {
    format!(
        "{{\"path\":{},\"kind\":{},\"old_schema_id\":{},\"new_schema_id\":{},\"migration\":{},\"reason\":{}}}",
        json_value(&fact.path),
        json_value(fact.kind.as_str()),
        fact.old_schema_id
            .as_deref()
            .map(json_value)
            .unwrap_or_else(|| "null".to_string()),
        fact.new_schema_id
            .as_deref()
            .map(json_value)
            .unwrap_or_else(|| "null".to_string()),
        json_value(fact.migration.as_str()),
        json_value(&fact.reason),
    )
}

pub(crate) fn render_json(value: &DataTree) -> String {
    match value {
        DataTree::Null => "null".to_string(),
        DataTree::Bool(value) => value.to_string(),
        DataTree::Int(value) => value.to_string(),
        DataTree::Float(value) => value.to_string(),
        DataTree::Number(value) => value.clone(),
        DataTree::TypedText(value) | DataTree::Text(value) => json_value(value),
        DataTree::Bytes(values) => format!(
            "[{}]",
            values
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
                .join(",")
        ),
        DataTree::Array(values) => format!(
            "[{}]",
            values.iter().map(render_json).collect::<Vec<_>>().join(",")
        ),
        DataTree::Object(values) => format!(
            "{{{}}}",
            values
                .iter()
                .map(|(key, value)| format!("{}:{}", json_value(key), render_json(value)))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}
fn object_map(value: &DataTree) -> Option<BTreeMap<String, DataTree>> {
    match value {
        DataTree::Object(values) => Some(values.iter().cloned().collect()),
        _ => None,
    }
}

fn request_string(text: &str, key: &str) -> String {
    let Ok(value) = parse_json(text) else {
        return String::new();
    };
    object_map(&value)
        .and_then(|object| object.get(key).and_then(json_string).map(str::to_string))
        .unwrap_or_default()
}

fn request_bool(text: &str, key: &str) -> bool {
    let Ok(value) = parse_json(text) else {
        return false;
    };
    object_map(&value)
        .and_then(|object| object.get(key).cloned())
        .is_some_and(|value| matches!(value, DataTree::Bool(true)))
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

    fn find(value: &DataTree, key: &str) -> Option<String> {
        match value {
            DataTree::Object(object) => {
                if let Some(DataTree::Text(value) | DataTree::TypedText(value)) =
                    object.iter().find_map(|(name, value)| (name == key).then_some(value))
                {
                    return Some(value.clone());
                }
                object.iter().find_map(|(_, value)| find(value, key))
            }
            DataTree::Array(values) => values.iter().find_map(|value| find(value, key)),
            _ => None,
        }
    }

    find(&value, key)
}

fn find_debugger_snapshot(text: &str) -> Option<DebuggerSnapshot> {
    let value = parse_json(text).ok()?;

    fn find(value: &DataTree) -> Option<DebuggerSnapshot> {
        match value {
            DataTree::Object(object) => {
                if let Some(DataTree::Text(session_id) | DataTree::TypedText(session_id)) =
                    object.iter().find_map(|(name, value)| (name == "id").then_some(value))
                {
                    if session_id.starts_with("canvas-debug-") {
                        let field = |key: &str| {
                            object
                                .iter()
                                .find_map(|(name, value)| (name == key).then_some(value))
                                .and_then(|value| match value {
                                    DataTree::Text(value) | DataTree::TypedText(value) => {
                                        Some(value.clone())
                                    }
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
                object.iter().find_map(|(_, value)| find(value))
            }
            DataTree::Array(values) => values.iter().find_map(find),
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
