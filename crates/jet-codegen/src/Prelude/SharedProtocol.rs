// D-SHAREDGUARD1=A: the one lock and condition protocol shared by native
// Prelude values and evaluator adapters. Payload storage is deliberately
// outside this module; engines may marshal values, but not redefine policy.

#[allow(dead_code)]
pub const JET_SHARED_GUARD_EDIT_REQUIRED: &str = "a condition wait needs an edit guard";
#[allow(dead_code)]
pub const JET_SHARED_GUARD_WAIT_CANCELLED: &str = "condition wait cancelled";
#[allow(dead_code)]
pub const JET_SHARED_GUARD_INVALID: &str = "SharedGuard is invalid or released";
#[allow(dead_code)]
pub const JET_SHARED_GUARD_VALUE_STORAGE_FAILED: &str = "SharedGuard value storage failed";
#[allow(dead_code)]
pub const JET_SHARED_GUARD_CHARACTER_STORAGE_FAILED: &str =
    "SharedGuard character storage failed";
#[allow(dead_code)]
pub const JET_SHARED_TRANSACTION_VALUE_STORAGE_FAILED: &str =
    "Shared transaction record payload became invalid";

pub fn jet_shared_guard_validate_char(value: i32) -> Result<char, &'static str> {
    char::from_u32(value as u32).ok_or(JET_SHARED_GUARD_CHARACTER_STORAGE_FAILED)
}

#[derive(Default)]
struct JetSharedLockState {
    readers: usize,
    writer: bool,
    writers_waiting: usize,
}

pub struct JetSharedProtocol {
    state: std::sync::Mutex<JetSharedLockState>,
    wake: std::sync::Condvar,
}

impl JetSharedProtocol {
    pub fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            state: std::sync::Mutex::new(JetSharedLockState::default()),
            wake: std::sync::Condvar::new(),
        })
    }

    pub fn acquire(
        self: &std::sync::Arc<Self>,
        editable: bool,
        mut cancelled: impl FnMut() -> bool,
    ) -> Option<std::sync::Arc<JetSharedPermit>> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if editable {
            state.writers_waiting += 1;
        }
        loop {
            if cancelled() {
                if editable {
                    state.writers_waiting -= 1;
                    self.wake.notify_all();
                }
                return None;
            }
            let available = if editable {
                !state.writer && state.readers == 0
            } else {
                !state.writer && state.writers_waiting == 0
            };
            if available {
                if editable {
                    state.writers_waiting -= 1;
                    state.writer = true;
                } else {
                    state.readers += 1;
                }
                return Some(std::sync::Arc::new(JetSharedPermit {
                    protocol: self.clone(),
                    editable,
                    held: std::sync::atomic::AtomicBool::new(true),
                }));
            }
            let (next, _) = self
                .wake
                .wait_timeout(state, std::time::Duration::from_millis(10))
                .unwrap_or_else(|error| error.into_inner());
            state = next;
        }
    }
}

pub fn jet_shared_acquire(
    protocol: &std::sync::Arc<JetSharedProtocol>,
    editable: bool,
    cancelled: impl FnMut() -> bool,
) -> Option<std::sync::Arc<JetSharedPermit>> {
    protocol.acquire(editable, cancelled)
}

pub fn jet_shared_acquire_ordered(
    mut protocols: Vec<std::sync::Arc<JetSharedProtocol>>,
) -> Vec<std::sync::Arc<JetSharedPermit>> {
    protocols.sort_unstable_by_key(|protocol| std::sync::Arc::as_ptr(protocol) as usize);
    protocols.dedup_by(|left, right| std::sync::Arc::ptr_eq(left, right));
    protocols
        .into_iter()
        .map(|protocol| {
            jet_shared_acquire(&protocol, true, || false)
                .expect("uncancelled transaction lock acquires")
        })
        .collect()
}

/// The Shared side of a `#Transact` block.
///
/// Engines and generated adapters only supply type-erased payload closures.
/// Participant identity, canonical lock ordering, commit, and rollback live
/// here so every execution tier uses one transaction protocol.
pub struct JetSharedTransaction {
    parts: Option<Vec<JetSharedTransactionPart>>,
}

struct JetSharedTransactionPart {
    protocol: std::sync::Arc<JetSharedProtocol>,
    deltas: Vec<Box<dyn FnOnce()>>,
}

pub fn jet_shared_transaction_begin() -> JetSharedTransaction {
    JetSharedTransaction {
        parts: Some(Vec::new()),
    }
}

impl JetSharedTransaction {
    pub fn touch(&mut self, protocol: std::sync::Arc<JetSharedProtocol>) {
        let parts = self
            .parts
            .as_mut()
            .expect("Shared transaction touch after commit");
        if !parts
            .iter()
            .any(|part| std::sync::Arc::ptr_eq(&part.protocol, &protocol))
        {
            parts.push(JetSharedTransactionPart {
                protocol,
                deltas: Vec::new(),
            });
        }
    }

    pub fn record_edit(
        &mut self,
        protocol: std::sync::Arc<JetSharedProtocol>,
        delta: Box<dyn FnOnce()>,
    ) {
        let parts = self
            .parts
            .as_mut()
            .expect("Shared transaction edit after commit");
        if let Some(part) = parts
            .iter_mut()
            .find(|part| std::sync::Arc::ptr_eq(&part.protocol, &protocol))
        {
            part.deltas.push(delta);
        } else {
            parts.push(JetSharedTransactionPart {
                protocol,
                deltas: vec![delta],
            });
        }
    }

    pub fn commit(mut self) {
        let _ = self.commit_with(|| ());
    }

    pub fn commit_with<R>(mut self, apply: impl FnOnce() -> R) -> R {
        let Some(mut parts) = self.parts.take() else {
            return apply();
        };
        let _permits = jet_shared_acquire_ordered(
            parts
                .iter()
                .map(|part| part.protocol.clone())
                .collect(),
        );
        for part in &mut parts {
            for delta in part.deltas.drain(..) {
                delta();
            }
        }
        apply()
    }
}

impl Drop for JetSharedTransaction {
    fn drop(&mut self) {
        // An uncommitted transaction drops its deferred payloads. No engine
        // gets a second rollback path to implement or accidentally invoke.
        self.parts.take();
    }
}

pub struct JetSharedPermit {
    protocol: std::sync::Arc<JetSharedProtocol>,
    editable: bool,
    held: std::sync::atomic::AtomicBool,
}

impl JetSharedPermit {
    pub fn editable(&self) -> bool {
        self.editable
    }

    pub fn held(&self) -> bool {
        self.held.load(std::sync::atomic::Ordering::Acquire)
    }

    pub fn release(&self) {
        if !self
            .held
            .swap(false, std::sync::atomic::Ordering::AcqRel)
        {
            return;
        }
        let mut state = self
            .protocol
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if self.editable {
            state.writer = false;
        } else {
            state.readers = state.readers.saturating_sub(1);
        }
        self.protocol.wake.notify_all();
    }

    pub fn reacquire(&self, mut cancelled: impl FnMut() -> bool) -> bool {
        if self.held() {
            return true;
        }
        let mut state = self
            .protocol
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if self.editable {
            state.writers_waiting += 1;
        }
        loop {
            if cancelled() {
                if self.editable {
                    state.writers_waiting -= 1;
                    self.protocol.wake.notify_all();
                }
                return false;
            }
            let available = if self.editable {
                !state.writer && state.readers == 0
            } else {
                !state.writer && state.writers_waiting == 0
            };
            if available {
                if self.editable {
                    state.writers_waiting -= 1;
                    state.writer = true;
                } else {
                    state.readers += 1;
                }
                self.held
                    .store(true, std::sync::atomic::Ordering::Release);
                return true;
            }
            let (next, _) = self
                .protocol
                .wake
                .wait_timeout(state, std::time::Duration::from_millis(10))
                .unwrap_or_else(|error| error.into_inner());
            state = next;
        }
    }
}

impl Drop for JetSharedPermit {
    fn drop(&mut self) {
        self.release();
    }
}

pub struct JetSharedGuardState {
    permit: std::sync::Arc<JetSharedPermit>,
    path: Vec<i64>,
    editable: bool,
    active: std::sync::atomic::AtomicBool,
}

impl JetSharedGuardState {
    pub fn permit_arc(&self) -> std::sync::Arc<JetSharedPermit> {
        self.permit.clone()
    }

    pub fn permit(&self) -> &JetSharedPermit {
        self.permit.as_ref()
    }

    pub fn editable(&self) -> bool {
        self.editable
    }

    pub fn held(&self) -> bool {
        self.active.load(std::sync::atomic::Ordering::Acquire) && self.permit.held()
    }

    pub fn path(&self) -> &[i64] {
        &self.path
    }
}

pub fn jet_shared_guard_acquire(
    protocol: &std::sync::Arc<JetSharedProtocol>,
    editable: bool,
    cancelled: impl FnMut() -> bool,
) -> Option<std::sync::Arc<JetSharedGuardState>> {
    jet_shared_acquire(protocol, editable, cancelled).map(|permit| {
        std::sync::Arc::new(JetSharedGuardState {
            permit,
            path: Vec::new(),
            editable,
            active: std::sync::atomic::AtomicBool::new(true),
        })
    })
}

pub fn jet_shared_guard_map(
    guard: &JetSharedGuardState,
    field: i64,
    editable: bool,
) -> Result<std::sync::Arc<JetSharedGuardState>, &'static str> {
    if !guard.held() {
        return Err(JET_SHARED_GUARD_INVALID);
    }
    if editable {
        jet_shared_guard_require_edit_capability(guard.editable(), guard.permit())?;
    }

    let mut path = guard.path.clone();
    path.push(field);
    let mapped = std::sync::Arc::new(JetSharedGuardState {
        permit: std::sync::Arc::clone(&guard.permit),
        path,
        editable,
        active: std::sync::atomic::AtomicBool::new(true),
    });
    guard
        .active
        .store(false, std::sync::atomic::Ordering::Release);
    Ok(mapped)
}

pub fn jet_shared_guard_clone(
    guard: &JetSharedGuardState,
    editable: bool,
) -> Result<std::sync::Arc<JetSharedGuardState>, &'static str> {
    if !guard.held() {
        return Err(JET_SHARED_GUARD_INVALID);
    }
    if editable {
        jet_shared_guard_require_edit_capability(guard.editable(), guard.permit())?;
    }

    Ok(std::sync::Arc::new(JetSharedGuardState {
        permit: std::sync::Arc::clone(&guard.permit),
        path: guard.path.clone(),
        editable,
        active: std::sync::atomic::AtomicBool::new(true),
    }))
}

pub fn jet_shared_guard_require_edit(
    guard: &JetSharedGuardState,
) -> Result<(), &'static str> {
    if !guard.held() {
        return Err(JET_SHARED_GUARD_INVALID);
    }
    jet_shared_guard_require_edit_capability(guard.editable(), guard.permit())
}

pub fn jet_shared_guard_require_edit_capability(
    editable: bool,
    permit: &JetSharedPermit,
) -> Result<(), &'static str> {
    if !permit.held() {
        return Err(JET_SHARED_GUARD_INVALID);
    }
    if !editable || !permit.editable() {
        return Err(JET_SHARED_GUARD_EDIT_REQUIRED);
    }
    Ok(())
}

pub trait JetConditionWaiter: Send + Sync {
    fn park(&self) -> Result<(), ()>;
    fn wake(&self);
    fn interrupted(&self) -> bool {
        false
    }
}

pub struct JetConditionProtocol {
    waiters: std::sync::Mutex<
        std::collections::VecDeque<(u64, std::sync::Arc<dyn JetConditionWaiter>)>,
    >,
    next_id: std::sync::atomic::AtomicU64,
    epoch: std::sync::atomic::AtomicU64,
    pending: std::sync::atomic::AtomicU64,
}

impl JetConditionProtocol {
    pub fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            waiters: std::sync::Mutex::new(std::collections::VecDeque::new()),
            next_id: std::sync::atomic::AtomicU64::new(1),
            epoch: std::sync::atomic::AtomicU64::new(0),
            pending: std::sync::atomic::AtomicU64::new(0),
        })
    }

    pub fn register(
        self: &std::sync::Arc<Self>,
        waiter: std::sync::Arc<dyn JetConditionWaiter>,
    ) -> JetConditionRegistration {
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut waiters = self
            .waiters
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let epoch = self
            .epoch
            .load(std::sync::atomic::Ordering::Relaxed);
        let stale = self
            .pending
            .fetch_update(
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
                |pending| {
                    if pending > 0 {
                        Some(pending - 1)
                    } else {
                        None
                    }
                },
            )
            .is_ok();
        waiters.push_back((id, waiter));
        JetConditionRegistration {
            condition: self.clone(),
            id,
            epoch,
            stale,
        }
    }

    pub fn notify_one(&self) {
        let waiter = {
            let mut waiters = self
                .waiters
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            self.epoch
                .fetch_add(1, std::sync::atomic::Ordering::Release);
            let waiter = waiters.pop_front().map(|(_, waiter)| waiter);
            if waiter.is_none() {
                self.pending.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            waiter
        };
        if let Some(waiter) = waiter {
            waiter.wake();
        }
    }

    pub fn notify_all(&self) {
        let waiters = {
            let mut registered = self
                .waiters
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            self.epoch
                .fetch_add(1, std::sync::atomic::Ordering::Release);
            let waiters = registered
                .drain(..)
                .map(|(_, waiter)| waiter)
                .collect::<Vec<_>>();
            if waiters.is_empty() {
                self.pending.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            waiters
        };
        for waiter in waiters {
            waiter.wake();
        }
    }

    fn unregister(&self, id: u64) {
        self.waiters
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .retain(|(candidate, _)| *candidate != id);
    }
}

pub fn jet_shared_condition_notify_one(
    condition: &std::sync::Arc<JetConditionProtocol>,
) {
    condition.notify_one();
}

pub fn jet_shared_condition_notify_all(
    condition: &std::sync::Arc<JetConditionProtocol>,
) {
    condition.notify_all();
}

pub struct JetConditionRegistration {
    condition: std::sync::Arc<JetConditionProtocol>,
    id: u64,
    epoch: u64,
    stale: bool,
}

impl JetConditionRegistration {
    fn saw_notification(&self) -> bool {
        self.stale
            || self
                .condition
                .epoch
                .load(std::sync::atomic::Ordering::Acquire)
                != self.epoch
    }
}

impl Drop for JetConditionRegistration {
    fn drop(&mut self) {
        self.condition.unregister(self.id);
    }
}

pub enum JetConditionWaitError<E> {
    Predicate(E),
    Cancelled,
}

struct JetConditionWaitCleanup<'a> {
    registration: Option<JetConditionRegistration>,
    permit: &'a JetSharedPermit,
    waiter: &'a dyn JetConditionWaiter,
    released: bool,
}

impl JetConditionWaitCleanup<'_> {
    fn release(&mut self) {
        self.permit.release();
        self.released = true;
    }

    fn finish(&mut self) -> Result<(), ()> {
        self.registration.take();
        if self.released {
            self.released = false;
            if !self.permit.reacquire(|| self.waiter.interrupted()) {
                return Err(());
            }
        }
        Ok(())
    }
}

impl Drop for JetConditionWaitCleanup<'_> {
    fn drop(&mut self) {
        self.registration.take();
        if self.released {
            let _ = self.permit.reacquire(|| self.waiter.interrupted());
        }
    }
}

fn jet_shared_condition_wait_registered(
    permit: &JetSharedPermit,
    registration: JetConditionRegistration,
    waiter: std::sync::Arc<dyn JetConditionWaiter>,
) -> Result<(), ()> {
    let saw_notification = registration.saw_notification();
    let mut cleanup = JetConditionWaitCleanup {
        registration: Some(registration),
        permit,
        waiter: waiter.as_ref(),
        released: false,
    };
    cleanup.release();
    let parked = if saw_notification {
        Ok(())
    } else {
        waiter.park()
    };
    let reacquired = cleanup.finish();
    drop(cleanup);
    parked.and(reacquired)
}

/// Park one condition-wait iteration after the caller checked its predicate.
/// The resident engines provide only the waiter adapter; release, registration,
/// reacquisition, and cleanup remain this Prelude protocol's policy.
pub fn jet_shared_condition_wait_once(
    permit: &JetSharedPermit,
    condition: &std::sync::Arc<JetConditionProtocol>,
    waiter: std::sync::Arc<dyn JetConditionWaiter>,
) -> Result<(), ()> {
    let registration = condition.register(waiter.clone());
    jet_shared_condition_wait_registered(permit, registration, waiter)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetSharedGuardWaitError {
    Invalid,
    EditRequired,
    Cancelled,
}

impl JetSharedGuardWaitError {
    pub fn message(self) -> &'static str {
        match self {
            Self::Invalid => JET_SHARED_GUARD_INVALID,
            Self::EditRequired => JET_SHARED_GUARD_EDIT_REQUIRED,
            Self::Cancelled => JET_SHARED_GUARD_WAIT_CANCELLED,
        }
    }

    pub fn traps(self) -> bool {
        matches!(self, Self::Invalid)
    }
}

pub fn jet_shared_guard_wait_once(
    guard: Option<&JetSharedGuardState>,
    condition: Option<&std::sync::Arc<JetConditionProtocol>>,
    waiter: std::sync::Arc<dyn JetConditionWaiter>,
) -> Result<(), JetSharedGuardWaitError> {
    let guard = guard.ok_or(JetSharedGuardWaitError::Invalid)?;
    let condition = condition.ok_or(JetSharedGuardWaitError::Invalid)?;
    jet_shared_guard_require_edit(guard).map_err(|message| {
        if message == JET_SHARED_GUARD_INVALID {
            JetSharedGuardWaitError::Invalid
        } else {
            JetSharedGuardWaitError::EditRequired
        }
    })?;
    jet_shared_condition_wait_once(guard.permit(), condition, waiter)
        .map_err(|_| JetSharedGuardWaitError::Cancelled)
}

pub fn jet_shared_condition_wait<E>(
    permit: &JetSharedPermit,
    condition: &std::sync::Arc<JetConditionProtocol>,
    mut ready: impl FnMut() -> Result<bool, E>,
    mut waiter: impl FnMut() -> std::sync::Arc<dyn JetConditionWaiter>,
) -> Result<(), JetConditionWaitError<E>> {
    loop {
        if ready().map_err(JetConditionWaitError::Predicate)? {
            return Ok(());
        }
        let waiter = waiter();
        let registration = condition.register(waiter.clone());
        if ready().map_err(JetConditionWaitError::Predicate)? {
            drop(registration);
            return Ok(());
        }
        let parked = jet_shared_condition_wait_registered(permit, registration, waiter);
        if parked.is_err() {
            return Err(JetConditionWaitError::Cancelled);
        }
    }
}

#[cfg(test)]
mod shared_protocol_tests {
    use super::*;

    struct CountingWaiter(std::sync::atomic::AtomicUsize);
    struct PanicWaiter;
    struct BlockingWaiter {
        notified: std::sync::atomic::AtomicBool,
        lock: std::sync::Mutex<()>,
        wake: std::sync::Condvar,
    }

    impl CountingWaiter {
        fn new() -> Self {
            Self(std::sync::atomic::AtomicUsize::new(0))
        }
    }

    impl JetConditionWaiter for CountingWaiter {
        fn park(&self) -> Result<(), ()> {
            Ok(())
        }

        fn wake(&self) {
            self.0
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    impl JetConditionWaiter for PanicWaiter {
        fn park(&self) -> Result<(), ()> {
            panic!("waiter panic")
        }

        fn wake(&self) {}
    }

    impl BlockingWaiter {
        fn new() -> Self {
            Self {
                notified: std::sync::atomic::AtomicBool::new(false),
                lock: std::sync::Mutex::new(()),
                wake: std::sync::Condvar::new(),
            }
        }
    }

    impl JetConditionWaiter for BlockingWaiter {
        fn park(&self) -> Result<(), ()> {
            if self
                .notified
                .swap(false, std::sync::atomic::Ordering::Acquire)
            {
                return Ok(());
            }
            let mut lock = self.lock.lock().unwrap();
            while !self
                .notified
                .swap(false, std::sync::atomic::Ordering::Acquire)
            {
                lock = self.wake.wait(lock).unwrap();
            }
            Ok(())
        }

        fn wake(&self) {
            let _lock = self.lock.lock().unwrap();
            self.notified
                .store(true, std::sync::atomic::Ordering::Release);
            self.wake.notify_one();
        }
    }

    #[test]
    fn ordered_acquisition_deduplicates_one_protocol() {
        let protocol = JetSharedProtocol::new();
        let permits =
            jet_shared_acquire_ordered(vec![protocol.clone(), protocol.clone()]);
        assert_eq!(permits.len(), 1);
        assert!(permits[0].editable());
    }

    #[test]
    fn notify_one_claims_each_waiter_once() {
        let condition = JetConditionProtocol::new();
        let first = std::sync::Arc::new(CountingWaiter::new());
        let second = std::sync::Arc::new(CountingWaiter::new());
        let _first_registration = condition.register(first.clone());
        let _second_registration = condition.register(second.clone());

        condition.notify_one();
        condition.notify_one();

        assert_eq!(
            first.0.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        assert_eq!(
            second.0.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
    }

    #[test]
    fn panic_during_park_unregisters_and_reacquires() {
        let protocol = JetSharedProtocol::new();
        let condition = JetConditionProtocol::new();
        let permit = protocol.acquire(true, || false).unwrap();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = jet_shared_condition_wait(
                &permit,
                &condition,
                || Ok::<bool, ()>(false),
                || std::sync::Arc::new(PanicWaiter),
            );
        }));

        assert!(result.is_err());
        assert!(permit.held());
        drop(permit);
        assert!(protocol.acquire(true, || false).is_some());
    }

    #[test]
    fn notify_before_registration_becomes_a_spurious_wake() {
        let protocol = JetSharedProtocol::new();
        let condition = JetConditionProtocol::new();
        let permit = protocol.acquire(true, || false).unwrap();
        condition.notify_one();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            jet_shared_condition_wait_once(
                &permit,
                &condition,
                std::sync::Arc::new(PanicWaiter),
            )
        }));

        assert!(result.is_ok());
        assert!(permit.held());
    }

    #[test]
    fn notify_between_registration_and_park_is_not_lost() {
        for _ in 0..64 {
            let protocol = JetSharedProtocol::new();
            let condition = JetConditionProtocol::new();
            let waiter = std::sync::Arc::new(BlockingWaiter::new());
            let rescue = waiter.clone();
            let ready = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let (registered_tx, registered_rx) = std::sync::mpsc::channel();
            let (park_tx, park_rx) = std::sync::mpsc::channel();
            let (done_tx, done_rx) = std::sync::mpsc::channel();
            let worker_condition = condition.clone();
            let worker_ready = ready.clone();
            let worker = std::thread::spawn(move || {
                let permit = protocol.acquire(true, || false).unwrap();
                let mut checks = 0usize;
                let result = jet_shared_condition_wait(
                    &permit,
                    &worker_condition,
                    || {
                        checks += 1;
                        if checks == 2 {
                            registered_tx.send(()).unwrap();
                            park_rx.recv().unwrap();
                            return Ok::<bool, ()>(false);
                        }
                        Ok(worker_ready.load(std::sync::atomic::Ordering::Acquire))
                    },
                    || waiter.clone(),
                );
                done_tx.send(result.is_ok()).unwrap();
            });

            registered_rx
                .recv_timeout(std::time::Duration::from_secs(1))
                .expect("waiter registers before the second predicate check");
            ready.store(true, std::sync::atomic::Ordering::Release);
            condition.notify_one();
            park_tx.send(()).unwrap();
            let done = done_rx.recv_timeout(std::time::Duration::from_secs(1));
            if done.is_err() {
                rescue.wake();
            }
            assert_eq!(done.ok(), Some(true));
            worker.join().unwrap();
        }
    }
}
