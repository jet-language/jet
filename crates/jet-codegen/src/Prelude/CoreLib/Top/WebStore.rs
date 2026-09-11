// D-DX-STORE1=A / #2476: typed Store over the canonical reactive signal.
//
// Store state has one JetSignal<T> reactive/read path; JetShared is the
// owner-bound opaque revision ticket for optimistic operations. Transactions
// publish one signal write and append one bounded, monotonic history entry.
// Scrubbing reads a historical snapshot; it never changes the live signal.
//
// The event stream is host-neutral metadata. A renderer or devtools adapter can
// marshal it into the canonical Store event without reimplementing store
// semantics. History and events are retained in development, and in the
// explicitly inspected read-only release profile only.

use std::any::{Any as JetWebStoreAny, TypeId as JetWebStoreTypeId};
use std::collections::HashMap as JetWebStoreMap;
use std::collections::VecDeque as JetWebStoreDeque;
use std::sync::{
    Arc as JetWebStoreArc,
    Condvar as JetWebStoreCondvar,
    LazyLock as JetWebStoreLazyLock,
    Mutex as JetWebStoreMutex,
};
use jet_foundation::Devtools::JetDevtoolsEvent as JetWebStoreDevtoolsEvent;
use std::time::{
    SystemTime as JetWebStoreSystemTime,
    UNIX_EPOCH as JetWebStoreUnixEpoch,
};

const JET_WEB_STORE_DEFAULT_HISTORY_LIMIT: usize = 1000;
// Store metadata is bounded independently from typed state. These limits keep
// registry keys, field labels, and retained timelines finite in every tier.
const JET_WEB_STORE_MAX_NAME: usize = 256;
const JET_WEB_STORE_MAX_REGISTRY: usize = 256;
const JET_WEB_STORE_MAX_HISTORY_LIMIT: usize = 1000;
fn jet_web_store_now_ms() -> u64 {
    JetWebStoreSystemTime::now()
        .duration_since(JetWebStoreUnixEpoch)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

fn jet_web_store_json(value: &str) -> String {
    format!("{value:?}")
}

fn jet_web_store_event_entity(name: &str) -> String {
    if !name.is_empty()
        && name.len() <= JET_WEB_STORE_MAX_NAME
        && !name.chars().any(char::is_control)
    {
        name.to_string()
    } else {
        "store".to_string()
    }
}

fn jet_web_store_publish_event(name: &str, event: &JetWebStoreEvent) {
    if !jet_web_runtime_devtools_enabled() {
        return;
    }
    let fields = format!(
        "{{\"name\":{},\"transaction\":{},\"diff\":{},\"cursor\":{},\"generation\":{},\"kind\":{}}}",
        jet_web_store_json(name),
        jet_web_store_json(&event.transaction_label()),
        event.diff_json(),
        event.cursor,
        event.generation,
        jet_web_store_json(event.kind.as_str()),
    );
    if let Ok(protocol_event) = JetWebStoreDevtoolsEvent::from_parts(
        jet_web_store_now_ms(),
        "core.web.store",
        "Store",
        jet_web_store_event_entity(name),
        fields,
    ) {
        jet_devtools_publish_event(protocol_event);
    }
}

const JET_WEB_STORE_MAX_FIELD: usize = 256;
const JET_WEB_STORE_MAX_FIELDS: usize = 256;

/// The release policy's read-only inspection profile is the only release mode
/// that keeps the store timeline. Local inspection has no Store panel code, so
/// retaining a private history there would be both waste and a policy leak.
#[inline(always)]
fn jet_web_store_history_capture_enabled() -> bool {
    jet_web_runtime_history_enabled()
}

type JetWebStoreRegistry = JetWebStoreMap<
    (JetWebStoreTypeId, String),
    Box<dyn JetWebStoreAny + Send + Sync>,
>;

static JET_WEB_STORE_REGISTRY: JetWebStoreLazyLock<JetWebStoreMutex<JetWebStoreRegistry>> =
    JetWebStoreLazyLock::new(|| JetWebStoreMutex::new(JetWebStoreMap::new()));

fn jet_web_store_registry() -> &'static JetWebStoreMutex<JetWebStoreRegistry> {
    &JET_WEB_STORE_REGISTRY
}

/// A re-entrant per-store publication gate. The outermost holder drains
/// deferred events after the signal write, so nested actions cannot expose
/// half-published state or reorder one transaction's receipt.
struct JetWebStoreGate {
    state: JetWebStoreMutex<JetWebStoreGateState>,
    wake: JetWebStoreCondvar,
}

struct JetWebStoreGateState {
    owner: Option<std::thread::ThreadId>,
    depth: usize,
    events: Vec<(String, JetWebStoreEvent)>,
}

struct JetWebStoreGateGuard {
    gate: JetWebStoreArc<JetWebStoreGate>,
}

impl JetWebStoreGate {
    fn new() -> Self {
        Self {
            state: JetWebStoreMutex::new(JetWebStoreGateState {
                owner: None,
                depth: 0,
                events: Vec::new(),
            }),
            wake: JetWebStoreCondvar::new(),
        }
    }

    fn lock(self: &JetWebStoreArc<Self>) -> JetWebStoreGateGuard {
        let owner = std::thread::current().id();
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        loop {
            match state.owner {
                None => {
                    state.owner = Some(owner);
                    state.depth = 1;
                    break;
                }
                Some(current) if current == owner => {
                    state.depth += 1;
                    break;
                }
                Some(_) => {
                    state = self
                        .wake
                        .wait(state)
                        .unwrap_or_else(|error| error.into_inner());
                }
            }
        }
        JetWebStoreGateGuard { gate: self.clone() }
    }

    fn defer_event(&self, name: String, event: JetWebStoreEvent) {
        let owner = std::thread::current().id();
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        debug_assert_eq!(state.owner, Some(owner));
        state.events.push((name, event));
    }
}

impl Drop for JetWebStoreGateGuard {
    fn drop(&mut self) {
        let owner = std::thread::current().id();
        let events = {
            let mut state = self
                .gate
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            debug_assert_eq!(state.owner, Some(owner));
            if state.depth > 1 {
                state.depth -= 1;
                Vec::new()
            } else {
                state.owner = None;
                state.depth = 0;
                let events = std::mem::take(&mut state.events);
                self.gate.wake.notify_all();
                events
            }
        };
        for (name, event) in events {
            jet_web_store_publish_event(&name, &event);
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetWebStoreTransaction<T> {
    pub generation: u64,
    pub action: String,
    pub changed_fields: Vec<String>,
    pub before: T,
    pub after: T,
}

impl<T> JetWebStoreTransaction<T> {
    /// The generic state stays typed; this deterministic metadata projection is
    /// the diff representation used by host/devtools adapters.
    pub fn diff_json(&self) -> String {
        let fields = self
            .changed_fields
            .iter()
            .map(|field| format!("\"{}\"", jet_web_store_json_escape(field)))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"generation\":{},\"action\":\"{}\",\"changed_fields\":[{}]}}",
            self.generation,
            jet_web_store_json_escape(&self.action),
            fields,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetWebStoreEventKind {
    Transaction,
    Navigation,
    Rollback,
}

impl JetWebStoreEventKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Transaction => "transaction",
            Self::Navigation => "navigation",
            Self::Rollback => "rollback",
        }
    }
}

/// Bounded, deterministic metadata emitted for every observable store change.
/// It intentionally does not erase or serialize the typed state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetWebStoreEvent {
    pub sequence: u64,
    pub generation: u64,
    pub action: String,
    pub changed_fields: Vec<String>,
    pub cursor: u64,
    pub kind: JetWebStoreEventKind,
}

impl JetWebStoreEvent {
    pub fn transaction_label(&self) -> String {
        format!("{}#{}", self.action, self.generation)
    }

    pub fn diff_json(&self) -> String {
        let fields = self
            .changed_fields
            .iter()
            .map(|field| format!("\"{}\"", jet_web_store_json_escape(field)))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"kind\":\"{}\",\"generation\":{},\"action\":\"{}\",\"changed_fields\":[{}]}}",
            self.kind.as_str(),
            self.generation,
            jet_web_store_json_escape(&self.action),
            fields,
        )
    }
}

#[derive(Clone)]
pub struct JetWebStoreInspection<T> {
    pub value: T,
    pub generation: u64,
    pub cursor: u64,
    pub history: Vec<JetWebStoreTransaction<T>>,
    pub events: Vec<JetWebStoreEvent>,
    pub history_limit: usize,
    pub history_enabled: bool,
}

struct JetWebStoreLedger<T> {
    /// State before the first retained transaction. It is replaced when the
    /// timeline is explicitly cleared or disabled, so a fresh timeline remains
    /// navigable without retaining an unbounded prefix.
    initial: T,
    history: JetWebStoreDeque<JetWebStoreTransaction<T>>,
    events: JetWebStoreDeque<JetWebStoreEvent>,
    /// Cursor is a point between entries: zero is `initial`, len is the live
    /// tip. A normal write truncates the future branch before appending.
    cursor: usize,
    /// Public generation labels are event-sequence metadata; the opaque
    /// Shared revision remains the authority for stale snapshot checks.
    current_generation: u64,
    next_event_sequence: u64,
    history_limit: usize,
}

impl<T: Clone> JetWebStoreLedger<T> {
    fn new(initial: T, history_limit: usize) -> Self {
        Self {
            initial,
            history: JetWebStoreDeque::new(),
            events: JetWebStoreDeque::new(),
            cursor: 0,
            current_generation: 0,
            next_event_sequence: 0,
            history_limit,
        }
    }

    fn trim_history(&mut self) {
        while self.history.len() > self.history_limit {
            if let Some(removed) = self.history.pop_front() {
                // The evicted transaction's `after` is the state immediately
                // before the first retained entry, so navigation still has a
                // correct bounded base.
                self.initial = removed.after;
                self.cursor = self.cursor.saturating_sub(1);
            }
        }
        self.cursor = self.cursor.min(self.history.len());
        while self.events.len() > self.history_limit {
            self.events.pop_front();
        }
    }

    fn push_transaction(&mut self, transaction: JetWebStoreTransaction<T>) {
        if !jet_web_store_history_capture_enabled() || self.history_limit == 0 {
            return;
        }
        // A write after `back()` starts a new branch. Generations remain
        // monotonic; only the unreachable future snapshots are discarded.
        while self.history.len() > self.cursor {
            self.history.pop_back();
        }
        self.history.push_back(transaction);
        self.cursor = self.history.len();
        self.trim_history();
    }

    fn reserve_sequence(&mut self) -> u64 {
        self.next_event_sequence = self
            .next_event_sequence
            .checked_add(1)
            .expect("Store event sequence exhausted");
        self.next_event_sequence
    }

    fn record_event(
        &mut self,
        sequence: u64,
        kind: JetWebStoreEventKind,
        generation: u64,
        action: String,
        changed_fields: Vec<String>,
    ) -> Option<JetWebStoreEvent> {
        let retain = jet_web_store_history_capture_enabled() && self.history_limit > 0;
        if !retain && !jet_web_runtime_devtools_enabled() {
            return None;
        }
        let event = JetWebStoreEvent {
            sequence,
            generation,
            action,
            changed_fields,
            cursor: self.cursor as u64,
            kind,
        };
        if retain {
            self.events.push_back(event.clone());
            while self.events.len() > self.history_limit {
                self.events.pop_front();
            }
        }
        Some(event)
    }

    fn generation_at_cursor(&self) -> u64 {
        self.cursor
            .checked_sub(1)
            .and_then(|index| self.history.get(index))
            .map(|transaction| transaction.generation)
            .unwrap_or(0)
    }
}

#[derive(Clone)]
pub struct JetWebStore<T> {
    pub name: String,
    signal: jet_std::JetSignal<T>,
    /// Shared owns the opaque monotonic revision used by optimistic tickets.
    /// The signal remains the sole reactive state/read path.
    shared: jet_std::JetShared<T>,
    ledger: JetWebStoreArc<JetWebStoreMutex<JetWebStoreLedger<T>>>,
    transaction_lock: JetWebStoreArc<JetWebStoreGate>,
}

#[derive(Clone)]
pub struct JetWebStoreSubscription {
    effect: jet_std::JetReactiveEffect,
}

impl JetWebStoreSubscription {
    pub fn unsubscribe(&self) {
        self.effect.unsubscribe();
    }

    pub fn active(&self) -> bool {
        self.effect.active()
    }
}

pub struct JetWebStorePatch<T: 'static> {
    store: JetWebStore<T>,
    transaction: JetWebStoreTransaction<T>,
    /// Consuming this owner-bound snapshot makes stale rollback impossible.
    snapshot: Option<jet_std::JetSharedSnapshot<T, T>>,
    active: bool,
}
impl<T: Clone + Send + Sync + 'static> JetWebStorePatch<T> {
    pub fn generation(&self) -> u64 {
        self.transaction.generation
    }

    pub fn transaction(&self) -> JetWebStoreTransaction<T> {
        self.transaction.clone()
    }

    pub fn active(&self) -> bool {
        self.active
    }

    /// Confirm the optimistic write. The transaction is already published;
    /// commit only closes the patch handle and returns its receipt.
    pub fn commit(mut self) -> JetWebStoreTransaction<T> {
        self.active = false;
        self.transaction
    }

    /// Roll back only if this patch is still the current timeline head. A
    /// stale patch cannot clobber a later transaction or a time-travel move.
    pub fn rollback(mut self) -> Option<T> {
        if !self.active {
            return None;
        }
        self.active = false;
        let snapshot = self.snapshot.take()?;
        self.store.rollback_patch(&self.transaction, snapshot)
    }
}

impl<T: Clone + Send + Sync + 'static> JetWebStore<T> {
    pub fn new(name: String, initial: T, history_limit: i64) -> Self {
        let history_limit = jet_web_store_clamp_history_limit(history_limit);
        Self {
            name,
            signal: jet_std::JetSignal::new(initial.clone()),
            shared: jet_std::JetShared::new(initial.clone()),
            ledger: JetWebStoreArc::new(JetWebStoreMutex::new(JetWebStoreLedger::new(
                initial,
                history_limit,
            ))),
            transaction_lock: JetWebStoreArc::new(JetWebStoreGate::new()),
        }
    }

    /// The current root value. Reads inside an Effect/Computed automatically
    /// subscribe to the same canonical reactive signal as every store update.
    pub fn value(&self) -> T {
        let _transaction_lock = self.transaction_lock.lock();
        self.signal.get()
    }

    pub fn signal(&self) -> jet_std::JetSignal<T> {
        self.signal.clone()
    }

    pub fn state_signal(&self) -> jet_std::JetSignal<T> {
        self.signal()
    }

    pub fn history_enabled(&self) -> bool {
        jet_web_store_history_capture_enabled()
    }

    pub fn history_limit(&self) -> i64 {
        let _transaction_lock = self.transaction_lock.lock();
        let ledger = self.ledger.lock().unwrap_or_else(|error| error.into_inner());
        i64::try_from(ledger.history_limit).unwrap_or(i64::MAX)
    }

    pub fn set_history_limit(&self, history_limit: i64) {
        let _transaction_lock = self.transaction_lock.lock();
        let history_limit = jet_web_store_clamp_history_limit(history_limit);
        let current = self.signal.get();
        let mut ledger = self.ledger.lock().unwrap_or_else(|error| error.into_inner());
        ledger.history_limit = history_limit;
        if history_limit == 0 || !jet_web_store_history_capture_enabled() {
            ledger.initial = current;
            ledger.history.clear();
            ledger.events.clear();
            ledger.cursor = 0;
        } else {
            // A limit change while the cursor is in the past must not leave a
            // retained future whose `before` state is no longer reachable.
            // Keep the current point and drop that unreachable branch before
            // applying front eviction.
            if ledger.cursor < ledger.history.len() && history_limit < ledger.history.len() {
                while ledger.history.len() > ledger.cursor {
                    ledger.history.pop_back();
                }
            }
            ledger.trim_history();
        }
    }

    /// Apply one update and publish exactly one reactive write. The canonical
    /// signal transaction exposes staged reads to the callback, while Shared
    /// owns the opaque revision used by optimistic snapshots.
    pub fn transact<F>(
        &self,
        action: String,
        changed_fields: Vec<String>,
        update: F,
    ) -> JetWebStoreTransaction<T>
    where
        F: FnOnce(&mut T),
    {
        let _transaction_lock = self.transaction_lock.lock();
        let normalized_action = jet_web_store_action(action);
        let normalized_fields = jet_web_store_fields(changed_fields);
        let (_, (transaction, event)) = jet_std::jet_reactive_transaction(
            &self.signal,
            |state| {
                let before = state.clone();
                update(state);
                let after = state.clone();
                self.shared.replace(after.clone());
                let mut ledger = self.ledger.lock().unwrap_or_else(|error| error.into_inner());
                let sequence = ledger.reserve_sequence();
                ledger.current_generation = sequence;
                let transaction = JetWebStoreTransaction {
                    generation: sequence,
                    action: normalized_action.clone(),
                    changed_fields: normalized_fields.clone(),
                    before,
                    after,
                };
                ledger.push_transaction(transaction.clone());
                let event = ledger.record_event(
                    sequence,
                    JetWebStoreEventKind::Transaction,
                    transaction.generation,
                    transaction.action.clone(),
                    transaction.changed_fields.clone(),
                );
                (transaction, event)
            },
        );
        if let Some(event) = event {
            self.transaction_lock
                .defer_event(self.name.clone(), event);
        }
        transaction
    }

    pub fn update<F>(
        &self,
        action: String,
        changed_fields: Vec<String>,
        update: F,
    ) -> JetWebStoreTransaction<T>
    where
        F: FnOnce(&mut T),
    {
        self.transact(action, changed_fields, update)
    }

    pub fn batch<F>(
        &self,
        action: String,
        changed_fields: Vec<String>,
        update: F,
    ) -> JetWebStoreTransaction<T>
    where
        F: FnOnce(&mut T),
    {
        self.transact(action, changed_fields, update)
    }

    pub fn set(&self, value: T) -> JetWebStoreTransaction<T> {
        self.transact("set".to_string(), Vec::new(), |state| *state = value)
    }

    pub fn set_state(&self, value: T) -> JetWebStoreTransaction<T> {
        self.set(value)
    }

    pub fn optimistic<F>(
        &self,
        action: String,
        changed_fields: Vec<String>,
        update: F,
    ) -> JetWebStorePatch<T>
    where
        F: FnOnce(&mut T),
    {
        let _transaction_lock = self.transaction_lock.lock();
        let transaction = self.transact(action, changed_fields, update);
        let snapshot = self.shared.capture();
        JetWebStorePatch {
            store: self.clone(),
            transaction,
            snapshot: Some(snapshot),
            active: true,
        }
    }

    pub fn patch<F>(
        &self,
        action: String,
        changed_fields: Vec<String>,
        update: F,
    ) -> JetWebStorePatch<T>
    where
        F: FnOnce(&mut T),
    {
        self.optimistic(action, changed_fields, update)
    }

    fn rollback_patch(
        &self,
        transaction: &JetWebStoreTransaction<T>,
        snapshot: jet_std::JetSharedSnapshot<T, T>,
    ) -> Option<T> {
        let _transaction_lock = self.transaction_lock.lock();
        let (value, event) = {
            let mut ledger = self.ledger.lock().unwrap_or_else(|error| error.into_inner());
            if ledger.current_generation != transaction.generation {
                return None;
            }
            let latest_is_patch = ledger
                .history
                .back()
                .map(|latest| latest.generation == transaction.generation)
                .unwrap_or(true);
            if !latest_is_patch {
                return None;
            }
            let value = transaction.before.clone();
            let replaced = self
                .shared
                .try_replace(snapshot, value.clone())
                .unwrap_or(false);
            if !replaced {
                return None;
            }
            if ledger
                .history
                .back()
                .map(|latest| latest.generation == transaction.generation)
                .unwrap_or(false)
            {
                ledger.history.pop_back();
                ledger.cursor = ledger.cursor.min(ledger.history.len());
            }
            ledger.current_generation = ledger.generation_at_cursor();
            let action = format!("rollback:{}", transaction.action);
            let fields = transaction.changed_fields.clone();
            let sequence = ledger.reserve_sequence();
            let generation = ledger.current_generation;
            let event = ledger.record_event(
                sequence,
                JetWebStoreEventKind::Rollback,
                generation,
                action,
                fields,
            );
            (value, event)
        };
        self.signal.set(value.clone());
        if let Some(event) = event {
            self.transaction_lock
                .defer_event(self.name.clone(), event);
        }
        Some(value)
    }

    /// Read a retained snapshot without changing the live signal. Generation
    /// zero denotes the base of the current retained timeline.
    pub fn scrub(&self, generation: u64) -> Option<T> {
        let _transaction_lock = self.transaction_lock.lock();
        let ledger = self.ledger.lock().unwrap_or_else(|error| error.into_inner());
        if generation == 0 {
            return (jet_web_store_history_capture_enabled() && !ledger.history.is_empty())
                .then(|| ledger.initial.clone());
        }
        ledger
            .history
            .iter()
            .find(|transaction| transaction.generation == generation)
            .map(|transaction| transaction.after.clone())
    }

    pub fn history_at(&self, index: i64) -> Option<JetWebStoreTransaction<T>> {
        let _transaction_lock = self.transaction_lock.lock();
        let ledger = self.ledger.lock().unwrap_or_else(|error| error.into_inner());
        let index = usize::try_from(index).ok()?;
        ledger.history.get(index).cloned()
    }

    pub fn history(&self) -> Vec<JetWebStoreTransaction<T>> {
        let _transaction_lock = self.transaction_lock.lock();
        let ledger = self.ledger.lock().unwrap_or_else(|error| error.into_inner());
        ledger.history.iter().cloned().collect()
    }

    pub fn events(&self) -> Vec<JetWebStoreEvent> {
        let _transaction_lock = self.transaction_lock.lock();
        let ledger = self.ledger.lock().unwrap_or_else(|error| error.into_inner());
        ledger.events.iter().cloned().collect()
    }

    pub fn events_since(&self, sequence: u64) -> Vec<JetWebStoreEvent> {
        let _transaction_lock = self.transaction_lock.lock();
        let ledger = self.ledger.lock().unwrap_or_else(|error| error.into_inner());
        ledger
            .events
            .iter()
            .filter(|event| event.sequence > sequence)
            .cloned()
            .collect()
    }

    pub fn clear_history(&self) {
        let _transaction_lock = self.transaction_lock.lock();
        let current = self.signal.get();
        let mut ledger = self.ledger.lock().unwrap_or_else(|error| error.into_inner());
        ledger.initial = current;
        ledger.history.clear();
        ledger.events.clear();
        ledger.cursor = 0;
        // Keep the opaque Shared revision and its public generation label
        // unchanged while making the live value the new retained base.
    }

    pub fn cursor(&self) -> u64 {
        let _transaction_lock = self.transaction_lock.lock();
        let ledger = self.ledger.lock().unwrap_or_else(|error| error.into_inner());
        ledger.cursor as u64
    }

    pub fn current_generation(&self) -> u64 {
        let _transaction_lock = self.transaction_lock.lock();
        let ledger = self.ledger.lock().unwrap_or_else(|error| error.into_inner());
        ledger.current_generation
    }

    pub fn back(&self) -> Option<T> {
        self.navigate(JetWebStoreNavigation::Back)
    }

    pub fn forward(&self) -> Option<T> {
        self.navigate(JetWebStoreNavigation::Forward)
    }

    pub fn jump(&self, generation: u64) -> Option<T> {
        self.navigate(JetWebStoreNavigation::Jump(generation))
    }

    /// Explicit live navigation. Unlike `scrub`, this changes the signal and
    /// therefore invalidates subscribers exactly once, without a new tx entry.
    fn navigate(&self, navigation: JetWebStoreNavigation) -> Option<T> {
        let _transaction_lock = self.transaction_lock.lock();
        let (value, event) = {
            let mut ledger = self.ledger.lock().unwrap_or_else(|error| error.into_inner());
            if !jet_web_store_history_capture_enabled()
                || ledger.history_limit == 0
                || ledger.history.is_empty()
            {
                return None;
            }
            let current_cursor = ledger.cursor;
            let (target_cursor, value, generation, action) = match navigation {
                JetWebStoreNavigation::Back => {
                    let target_cursor = current_cursor.checked_sub(1)?;
                    let value = if target_cursor == 0 {
                        ledger.initial.clone()
                    } else {
                        ledger.history.get(target_cursor - 1)?.after.clone()
                    };
                    let generation = if target_cursor == 0 {
                        0
                    } else {
                        ledger.history.get(target_cursor - 1)?.generation
                    };
                    (target_cursor, value, generation, "back".to_string())
                }
                JetWebStoreNavigation::Forward => {
                    let transaction = ledger.history.get(current_cursor)?;
                    (
                        current_cursor + 1,
                        transaction.after.clone(),
                        transaction.generation,
                        "forward".to_string(),
                    )
                }
                JetWebStoreNavigation::Jump(generation) => {
                    if generation == 0 {
                        (0, ledger.initial.clone(), 0, "jump:0".to_string())
                    } else {
                        let (index, transaction) = ledger
                            .history
                            .iter()
                            .enumerate()
                            .find(|(_, transaction)| transaction.generation == generation)?;
                        (
                            index + 1,
                            transaction.after.clone(),
                            transaction.generation,
                            format!("jump:{generation}"),
                        )
                    }
                }
            };
            if target_cursor == current_cursor {
                return Some(value);
            }
            self.shared.replace(value.clone());
            ledger.cursor = target_cursor;
            ledger.current_generation = generation;
            let sequence = ledger.reserve_sequence();
            let event = ledger.record_event(
                sequence,
                JetWebStoreEventKind::Navigation,
                generation,
                action,
                Vec::new(),
            );
            (value, event)
        };
        self.signal.set(value.clone());
        if let Some(event) = event {
            self.transaction_lock
                .defer_event(self.name.clone(), event);
        }
        Some(value)
    }

    /// Restore is the explicit live counterpart to `scrub` and follows the
    /// retained timeline cursor rather than creating a synthetic transaction.
    pub fn restore(&self, generation: u64) -> Option<T> {
        self.jump(generation)
    }

    pub fn subscribe<F>(&self, callback: F) -> JetWebStoreSubscription
    where
        F: Fn(T) + Send + Sync + 'static,
    {
        let signal = self.signal.clone();
        let effect = jet_std::jet_reactive_effect(move || callback(signal.get()));
        JetWebStoreSubscription { effect }
    }

    pub fn subscribe_selector<U, Select, F>(
        &self,
        selector: Select,
        callback: F,
    ) -> JetWebStoreSubscription
    where
        U: Send + 'static,
        Select: Fn(T) -> U + Send + Sync + 'static,
        F: Fn(U) + Send + Sync + 'static,
    {
        let signal = self.signal.clone();
        let effect = jet_std::jet_reactive_effect(move || callback(selector(signal.get())));
        JetWebStoreSubscription { effect }
    }

    pub fn derived<U, F>(&self, compute: F) -> jet_std::JetDerived<U>
    where
        U: Clone + Send + Sync + 'static,
        F: Fn(T) -> U + Send + Sync + 'static,
    {
        let signal = self.signal.clone();
        jet_std::JetDerived::new(move || compute(signal.get()))
    }

    pub fn selector<U, F>(&self, compute: F) -> jet_std::JetDerived<U>
    where
        U: Clone + Send + Sync + 'static,
        F: Fn(T) -> U + Send + Sync + 'static,
    {
        self.derived(compute)
    }

    pub fn inspect(&self) -> JetWebStoreInspection<T> {
        let _transaction_lock = self.transaction_lock.lock();
        let value = self.signal.get();
        let ledger = self.ledger.lock().unwrap_or_else(|error| error.into_inner());
        JetWebStoreInspection {
            value,
            generation: ledger.current_generation,
            cursor: ledger.cursor as u64,
            history: ledger.history.iter().cloned().collect(),
            events: ledger.events.iter().cloned().collect(),
            history_limit: ledger.history_limit,
            history_enabled: jet_web_store_history_capture_enabled(),
        }
    }

    /// Facts are deliberately payload-free. `event_json` gives the canonical
    /// Store body shape without serializing the generic root state.
    pub fn facts_json(&self) -> String {
        let _transaction_lock = self.transaction_lock.lock();
        let ledger = self.ledger.lock().unwrap_or_else(|error| error.into_inner());
        format!(
            "{{\"name\":\"{}\",\"generation\":{},\"cursor\":{},\"history_length\":{},\"history_limit\":{},\"event_length\":{},\"history_enabled\":{}}}",
            jet_web_store_json_escape(&self.name),
            ledger.current_generation,
            ledger.cursor,
            ledger.history.len(),
            ledger.history_limit,
            ledger.events.len(),
            jet_web_store_history_capture_enabled(),
        )
    }

    pub fn event_json(&self) -> String {
        let _transaction_lock = self.transaction_lock.lock();
        let ledger = self.ledger.lock().unwrap_or_else(|error| error.into_inner());
        let Some(event) = ledger.events.back() else {
            return format!(
                "{{\"name\":\"{}\",\"transaction\":\"\",\"diff\":\"{{}}\",\"cursor\":{}}}",
                jet_web_store_json_escape(&self.name),
                ledger.cursor,
            );
        };
        let diff = jet_web_store_json_escape(&event.diff_json());
        format!(
            "{{\"name\":\"{}\",\"transaction\":\"{}\",\"diff\":\"{}\",\"cursor\":{}}}",
            jet_web_store_json_escape(&self.name),
            jet_web_store_json_escape(&event.transaction_label()),
            diff,
            event.cursor,
        )
    }
}

enum JetWebStoreNavigation {
    Back,
    Forward,
    Jump(u64),
}

/// Named construction is the state-preserving path used by compatible reloads.
/// The key includes the concrete row type, so a type-changing edit never
/// reuses an incompatible value.
pub fn jet_web_store<T: Clone + Send + Sync + 'static>(
    name: String,
    initial: T,
) -> JetWebStore<T> {
    jet_web_store_with_history(name, initial, jet_web_store_default_history_limit())
}

pub fn jet_web_store_with_history<T: Clone + Send + Sync + 'static>(
    name: String,
    initial: T,
    history_limit: i64,
) -> JetWebStore<T> {
    let retain = !name.is_empty()
        && name.len() <= JET_WEB_STORE_MAX_NAME
        && !name.chars().any(|character| character.is_control());
    let key = (JetWebStoreTypeId::of::<T>(), name.clone());
    let mut registry = jet_web_store_registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(existing) = registry
        .get(&key)
        .and_then(|value| value.downcast_ref::<JetWebStore<T>>())
    {
        return existing.clone();
    }
    if !retain || registry.len() >= JET_WEB_STORE_MAX_REGISTRY {
        drop(registry);
        return JetWebStore::new(name, initial, history_limit);
    }
    let store = JetWebStore::new(name, initial, history_limit);
    registry.insert(key, Box::new(store.clone()));
    store
}

pub fn jet_web_store_transaction<T: Clone + Send + Sync + 'static, F>(
    store: &JetWebStore<T>,
    action: String,
    changed_fields: Vec<String>,
    update: F,
) -> JetWebStoreTransaction<T>
where
    F: FnOnce(&mut T),
{
    store.transact(action, changed_fields, update)
}
pub fn jet_web_store_value<T: Clone + Send + Sync + 'static>(store: &JetWebStore<T>) -> T {
    store.value()
}

pub fn jet_web_store_update<T: Clone + Send + Sync + 'static, F>(
    store: &JetWebStore<T>,
    action: String,
    changed_fields: Vec<String>,
    update: F,
) -> JetWebStoreTransaction<T>
where
    F: FnOnce(&mut T),
{
    store.update(action, changed_fields, update)
}

pub fn jet_web_store_batch<T: Clone + Send + Sync + 'static, F>(
    store: &JetWebStore<T>,
    action: String,
    changed_fields: Vec<String>,
    update: F,
) -> JetWebStoreTransaction<T>
where
    F: FnOnce(&mut T),
{
    store.batch(action, changed_fields, update)
}

pub fn jet_web_store_set<T: Clone + Send + Sync + 'static>(
    store: &JetWebStore<T>,
    value: T,
) -> JetWebStoreTransaction<T> {
    store.set(value)
}

pub fn jet_web_store_set_state<T: Clone + Send + Sync + 'static>(
    store: &JetWebStore<T>,
    value: T,
) -> JetWebStoreTransaction<T> {
    store.set_state(value)
}

pub fn jet_web_store_optimistic<T: Clone + Send + Sync + 'static, F>(
    store: &JetWebStore<T>,
    action: String,
    changed_fields: Vec<String>,
    update: F,
) -> JetWebStorePatch<T>
where
    F: FnOnce(&mut T),
{
    store.optimistic(action, changed_fields, update)
}

pub fn jet_web_store_patch<T: Clone + Send + Sync + 'static, F>(
    store: &JetWebStore<T>,
    action: String,
    changed_fields: Vec<String>,
    update: F,
) -> JetWebStorePatch<T>
where
    F: FnOnce(&mut T),
{
    store.patch(action, changed_fields, update)
}

pub fn jet_web_store_back<T: Clone + Send + Sync + 'static>(
    store: &JetWebStore<T>,
) -> Option<T> {
    store.back()
}

pub fn jet_web_store_forward<T: Clone + Send + Sync + 'static>(
    store: &JetWebStore<T>,
) -> Option<T> {
    store.forward()
}

pub fn jet_web_store_jump<T: Clone + Send + Sync + 'static>(
    store: &JetWebStore<T>,
    generation: i64,
) -> Option<T> {
    u64::try_from(generation).ok().and_then(|generation| store.jump(generation))
}

pub fn jet_web_store_scrub<T: Clone + Send + Sync + 'static>(
    store: &JetWebStore<T>,
    generation: i64,
) -> Option<T> {
    u64::try_from(generation).ok().and_then(|generation| store.scrub(generation))
}

pub fn jet_web_store_restore<T: Clone + Send + Sync + 'static>(
    store: &JetWebStore<T>,
    generation: i64,
) -> Option<T> {
    u64::try_from(generation)
        .ok()
        .and_then(|generation| store.restore(generation))
}

pub fn jet_web_store_history<T: Clone + Send + Sync + 'static>(
    store: &JetWebStore<T>,
) -> Vec<JetWebStoreTransaction<T>> {
    store.history()
}

pub fn jet_web_store_history_at<T: Clone + Send + Sync + 'static>(
    store: &JetWebStore<T>,
    index: i64,
) -> Option<JetWebStoreTransaction<T>> {
    store.history_at(index)
}

pub fn jet_web_store_events<T: Clone + Send + Sync + 'static>(
    store: &JetWebStore<T>,
) -> Vec<JetWebStoreEvent> {
    store.events()
}

pub fn jet_web_store_events_since<T: Clone + Send + Sync + 'static>(
    store: &JetWebStore<T>,
    sequence: i64,
) -> Vec<JetWebStoreEvent> {
    u64::try_from(sequence)
        .ok()
        .map_or_else(Vec::new, |sequence| store.events_since(sequence))
}

pub fn jet_web_store_clear_history<T: Clone + Send + Sync + 'static>(store: &JetWebStore<T>) {
    store.clear_history()
}

pub fn jet_web_store_history_enabled<T: Clone + Send + Sync + 'static>(
    store: &JetWebStore<T>,
) -> bool {
    store.history_enabled()
}

pub fn jet_web_store_history_limit<T: Clone + Send + Sync + 'static>(
    store: &JetWebStore<T>,
) -> i64 {
    store.history_limit()
}

pub fn jet_web_store_set_history_limit<T: Clone + Send + Sync + 'static>(
    store: &JetWebStore<T>,
    history_limit: i64,
) {
    store.set_history_limit(history_limit)
}

pub fn jet_web_store_cursor<T: Clone + Send + Sync + 'static>(store: &JetWebStore<T>) -> i64 {
    i64::try_from(store.cursor()).unwrap_or(i64::MAX)
}

pub fn jet_web_store_current_generation<T: Clone + Send + Sync + 'static>(
    store: &JetWebStore<T>,
) -> i64 {
    i64::try_from(store.current_generation()).unwrap_or(i64::MAX)
}

pub fn jet_web_store_subscribe<T: Clone + Send + Sync + 'static, F>(
    store: &JetWebStore<T>,
    callback: F,
) -> JetWebStoreSubscription
where
    F: Fn(T) + Send + Sync + 'static,
{
    store.subscribe(callback)
}

pub fn jet_web_store_subscribe_selector<
    T: Clone + Send + Sync + 'static,
    U: Send + 'static,
    Select,
    F,
>(
    store: &JetWebStore<T>,
    selector: Select,
    callback: F,
) -> JetWebStoreSubscription
where
    Select: Fn(T) -> U + Send + Sync + 'static,
    F: Fn(U) + Send + Sync + 'static,
{
    store.subscribe_selector(selector, callback)
}

pub fn jet_web_store_derived<T: Clone + Send + Sync + 'static, U: Clone + Send + Sync + 'static, F>(
    store: &JetWebStore<T>,
    compute: F,
) -> jet_std::JetDerived<U>
where
    F: Fn(T) -> U + Send + Sync + 'static,
{
    store.derived(compute)
}

pub fn jet_web_store_selector<T: Clone + Send + Sync + 'static, U: Clone + Send + Sync + 'static, F>(
    store: &JetWebStore<T>,
    compute: F,
) -> jet_std::JetDerived<U>
where
    F: Fn(T) -> U + Send + Sync + 'static,
{
    store.selector(compute)
}

pub fn jet_web_store_inspect<T: Clone + Send + Sync + 'static>(
    store: &JetWebStore<T>,
) -> JetWebStoreInspection<T> {
    store.inspect()
}

pub fn jet_web_store_facts_json<T: Clone + Send + Sync + 'static>(
    store: &JetWebStore<T>,
) -> String {
    store.facts_json()
}

pub fn jet_web_store_event_json<T: Clone + Send + Sync + 'static>(
    store: &JetWebStore<T>,
) -> String {
    store.event_json()
}
pub fn jet_web_store_signal<T: Clone + Send + Sync + 'static>(
    store: &JetWebStore<T>,
) -> jet_std::JetSignal<T> {
    store.signal()
}

pub fn jet_web_store_state_signal<T: Clone + Send + Sync + 'static>(
    store: &JetWebStore<T>,
) -> jet_std::JetSignal<T> {
    store.state_signal()
}

pub fn jet_web_store_patch_generation<T: Clone + Send + Sync + 'static>(
    patch: &JetWebStorePatch<T>,
) -> i64 {
    i64::try_from(patch.generation()).unwrap_or(i64::MAX)
}

pub fn jet_web_store_patch_transaction<T: Clone + Send + Sync + 'static>(
    patch: &JetWebStorePatch<T>,
) -> JetWebStoreTransaction<T> {
    patch.transaction()
}

pub fn jet_web_store_patch_active<T: Clone + Send + Sync + 'static>(
    patch: &JetWebStorePatch<T>,
) -> bool {
    patch.active()
}

pub fn jet_web_store_patch_commit<T: Clone + Send + Sync + 'static>(
    patch: JetWebStorePatch<T>,
) -> JetWebStoreTransaction<T> {
    patch.commit()
}

pub fn jet_web_store_patch_rollback<T: Clone + Send + Sync + 'static>(
    patch: JetWebStorePatch<T>,
) -> Option<T> {
    patch.rollback()
}

pub fn jet_web_store_subscription_unsubscribe(subscription: &JetWebStoreSubscription) {
    subscription.unsubscribe()
}

pub fn jet_web_store_subscription_active(subscription: &JetWebStoreSubscription) -> bool {
    subscription.active()
}

pub fn jet_web_store_transaction_diff<T>(
    transaction: &JetWebStoreTransaction<T>,
) -> String {
    transaction.diff_json()
}

pub fn jet_web_store_event_transaction_label(event: &JetWebStoreEvent) -> String {
    event.transaction_label()
}

pub fn jet_web_store_event_diff(event: &JetWebStoreEvent) -> String {
    event.diff_json()
}

fn jet_web_store_default_history_limit() -> i64 {
    JET_WEB_STORE_DEFAULT_HISTORY_LIMIT as i64
}

fn jet_web_store_clamp_history_limit(value: i64) -> usize {
    value
        .max(0)
        .try_into()
        .unwrap_or(usize::MAX)
        .min(JET_WEB_STORE_MAX_HISTORY_LIMIT)
}

fn jet_web_store_fields(fields: Vec<String>) -> Vec<String> {
    let mut result = Vec::new();
    for field in fields.into_iter().take(JET_WEB_STORE_MAX_FIELDS) {
        if !field.is_empty()
            && field.len() <= JET_WEB_STORE_MAX_FIELD
            && !field.chars().any(|character| character.is_control())
            && !result.iter().any(|existing| existing == &field)
        {
            result.push(field);
        }
    }
    // Changed-field order is a protocol fact, not caller-dependent presentation.
    result.sort_unstable();
    result
}
fn jet_web_store_action(action: String) -> String {
    let action = if action.is_empty() {
        "update".to_string()
    } else {
        action
    };
    let mut sanitized = String::new();
    for character in action.chars().filter(|character| !character.is_control()) {
        if sanitized.len() + character.len_utf8() > JET_WEB_STORE_MAX_FIELD {
            break;
        }
        sanitized.push(character);
    }
    if sanitized.is_empty() {
        "update".to_string()
    } else {
        sanitized
    }
}


fn jet_web_store_json_escape(value: &str) -> String {
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
