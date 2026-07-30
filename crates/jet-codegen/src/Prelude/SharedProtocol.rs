// D-SHAREDGUARD1=A: the one lock and condition protocol shared by native
// Prelude values and evaluator adapters. Payload storage is deliberately
// outside this module; engines may marshal values, but not redefine policy.

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
            if cancelled() {
                if editable {
                    state.writers_waiting -= 1;
                    self.wake.notify_all();
                }
                return None;
            }
            let (next, _) = self
                .wake
                .wait_timeout(state, std::time::Duration::from_millis(10))
                .unwrap_or_else(|error| error.into_inner());
            state = next;
        }
    }
}

pub fn jet_shared_acquire_ordered(
    mut protocols: Vec<std::sync::Arc<JetSharedProtocol>>,
) -> Vec<std::sync::Arc<JetSharedPermit>> {
    protocols.sort_unstable_by_key(|protocol| std::sync::Arc::as_ptr(protocol) as usize);
    protocols.dedup_by(|left, right| std::sync::Arc::ptr_eq(left, right));
    protocols
        .into_iter()
        .map(|protocol| {
            protocol
                .acquire(true, || false)
                .expect("uncancelled transaction lock acquires")
        })
        .collect()
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
            if cancelled() {
                if self.editable {
                    state.writers_waiting -= 1;
                    self.protocol.wake.notify_all();
                }
                return false;
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

pub trait JetConditionWaiter: Send + Sync {
    fn park(&self) -> Result<(), ()>;
    fn wake(&self);
}

pub struct JetConditionProtocol {
    waiters: std::sync::Mutex<
        std::collections::VecDeque<(u64, std::sync::Arc<dyn JetConditionWaiter>)>,
    >,
    next_id: std::sync::atomic::AtomicU64,
}

impl JetConditionProtocol {
    pub fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            waiters: std::sync::Mutex::new(std::collections::VecDeque::new()),
            next_id: std::sync::atomic::AtomicU64::new(1),
        })
    }

    pub fn register(
        self: &std::sync::Arc<Self>,
        waiter: std::sync::Arc<dyn JetConditionWaiter>,
    ) -> JetConditionRegistration {
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.waiters
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push_back((id, waiter));
        JetConditionRegistration {
            condition: self.clone(),
            id,
        }
    }

    pub fn notify_one(&self) {
        let waiter = self
            .waiters
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .pop_front()
            .map(|(_, waiter)| waiter);
        if let Some(waiter) = waiter {
            waiter.wake();
        }
    }

    pub fn notify_all(&self) {
        let waiters = self
            .waiters
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .drain(..)
            .map(|(_, waiter)| waiter)
            .collect::<Vec<_>>();
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

pub struct JetConditionRegistration {
    condition: std::sync::Arc<JetConditionProtocol>,
    id: u64,
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
    released: bool,
}

impl JetConditionWaitCleanup<'_> {
    fn release(&mut self) {
        self.permit.release();
        self.released = true;
    }
}

impl Drop for JetConditionWaitCleanup<'_> {
    fn drop(&mut self) {
        self.registration.take();
        if self.released {
            let _ = self.permit.reacquire(|| false);
        }
    }
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
        let mut cleanup = JetConditionWaitCleanup {
            registration: Some(registration),
            permit,
            released: false,
        };
        cleanup.release();
        let parked = waiter.park();
        drop(cleanup);
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
