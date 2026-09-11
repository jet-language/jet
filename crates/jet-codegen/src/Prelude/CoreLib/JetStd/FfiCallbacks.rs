    // D-FFI-CALLBACK2=A: a foreign callback is a managed registration, not a
    // bare function pointer. Admission, in-flight accounting, native shutdown
    // acknowledgement, and retained-state release live in this one runtime.
    // The generated facade supplies the contract and the native stop/release
    // hooks; no adapter may invent a second lifecycle protocol.
    use std::ffi::{c_char, c_void, CStr};
    use std::sync::atomic::{AtomicU8, AtomicUsize};
    use std::sync::Condvar;

    // `RefCell`, `AtomicU64`, `Ordering`, `Arc`, and `Mutex` are imported by
    // the preceding JetStd fragments. Keep this fragment's imports disjoint:
    // CoreLib splices every fragment into one `jet_std` module.

    const JET_FFI_CALLBACK_ACTIVE: u8 = 0;
    const JET_FFI_CALLBACK_STOPPING: u8 = 1;
    const JET_FFI_CALLBACK_STOPPED: u8 = 2;
    const JET_FFI_CALLBACK_QUARANTINED: u8 = 3;

    /// Native callback registration flags used by the generated C ABI glue.
    /// The bits are deliberately closed: an unknown or incomplete contract is
    /// rejected before the callback is made visible to a Jet program.
    pub const JET_FFI_CALLBACK_FLAG_CAPTURE_RETAINED: u32 = 1 << 0;
    pub const JET_FFI_CALLBACK_FLAG_THREAD_ATTACHED: u32 = 1 << 1;
    pub const JET_FFI_CALLBACK_FLAG_REENTRANT: u32 = 1 << 2;
    pub const JET_FFI_CALLBACK_FLAG_REGISTRATION_OWNED: u32 = 1 << 3;
    pub const JET_FFI_CALLBACK_FLAG_COMPLETION_ACK: u32 = 1 << 4;
    pub const JET_FFI_CALLBACK_FLAG_NATIVE_RETAINED: u32 = 1 << 5;
    pub const JET_FFI_CALLBACK_FLAG_FFI_C_EFFECT: u32 = 1 << 6;
    pub const JET_FFI_CALLBACK_FLAG_IO_WRITE_EFFECT: u32 = 1 << 7;
    pub const JET_FFI_CALLBACK_REQUIRED_FLAGS: u32 = JET_FFI_CALLBACK_FLAG_CAPTURE_RETAINED
        | JET_FFI_CALLBACK_FLAG_THREAD_ATTACHED
        | JET_FFI_CALLBACK_FLAG_REGISTRATION_OWNED
        | JET_FFI_CALLBACK_FLAG_COMPLETION_ACK
        | JET_FFI_CALLBACK_FLAG_NATIVE_RETAINED
        | JET_FFI_CALLBACK_FLAG_FFI_C_EFFECT
        | JET_FFI_CALLBACK_FLAG_IO_WRITE_EFFECT;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum JetFfiCallbackThreadAffinity {
        Any,
        Main,
        AttachedForeign,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct JetFfiCallbackContract {
        /// The callback closure's captures stay live until native shutdown ACK
        /// and every admitted invocation has left the trampoline.
        pub capture_lifetime: bool,
        /// `AttachedForeign` requires the vetted attachment guard on every
        /// foreign-thread entry. `Main` pins registration and invocation to one
        /// thread. `Any` is reserved for adapters that prove a stronger policy.
        pub thread_affinity: JetFfiCallbackThreadAffinity,
        pub reentrant: bool,
        pub registration_ownership: bool,
        pub completion: bool,
        pub retained_native_access: bool,
        pub authority_scope: bool,
        /// Canonical effect names, normally `FFI.C + IO.Write`.
        pub effects: String,
        pub callback_identity: String,
        pub plan_digest: String,
    }

    impl JetFfiCallbackContract {
        pub fn managed(identity: impl Into<String>, plan_digest: impl Into<String>) -> Self {
            Self {
                capture_lifetime: true,
                thread_affinity: JetFfiCallbackThreadAffinity::AttachedForeign,
                reentrant: true,
                registration_ownership: true,
                completion: true,
                retained_native_access: true,
                authority_scope: true,
                effects: "FFI.C + IO.Write".to_string(),
                callback_identity: identity.into(),
                plan_digest: plan_digest.into(),
            }
        }

        /// Validate every fact required before exposing a safe callback facade.
        /// This runs before registration, rather than discovering a missing
        /// lifetime or shutdown fact after native code has retained a pointer.
        pub fn validate(&self, has_release: bool) -> Result<(), String> {
            if self.callback_identity.trim().is_empty() {
                return Err("callback identity is empty".to_string());
            }
            if self.plan_digest.trim().is_empty() {
                return Err("callback plan digest is empty".to_string());
            }
            if !self.capture_lifetime {
                return Err("callback capture lifetime is not retained through shutdown".to_string());
            }
            if !self.registration_ownership {
                return Err("callback registration has no consuming owner".to_string());
            }
            if !self.completion {
                return Err("callback registration has no completion acknowledgement".to_string());
            }
            if !self.authority_scope {
                return Err("callback authority scope does not cover registration and cleanup".to_string());
            }
            if self.retained_native_access && !has_release {
                return Err("retained native callback access has no release hook".to_string());
            }
            if self.effects.split('+').map(str::trim).all(|part| part != "FFI.C") {
                return Err("callback effects omit FFI.C".to_string());
            }
            if self.effects.split('+').map(str::trim).all(|part| part != "IO.Write") {
                return Err("callback effects omit IO.Write".to_string());
            }
            Ok(())
        }
    }

    fn jet_ffi_callback_plan_digest(identity: &str) -> String {
        let input = format!("jet-ffi-callback-plan-v1\0{identity}");
        let mut digest = String::with_capacity(64);
        for byte in jet_sha256_raw(input.as_bytes()) {
            use std::fmt::Write;
            let _ = write!(digest, "{byte:02x}");
        }
        digest
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum JetFfiCallbackDispatch {
        Accepted,
        RejectedStopped,
        RejectedReentrant,
        RejectedThread,
        RejectedQuarantined,
        Failed(String),
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum JetFfiStopResult {
        Stopped,
        CallbackFailed(String),
        NativeBoundaryFailed(String),
        Quarantined(String),
    }

    thread_local! {
        static JET_FFI_CALLBACK_STACK: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
        static JET_FFI_RUNTIME_ATTACHED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        static JET_FFI_CURRENT_EVENT_STOP: RefCell<Vec<std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<dyn Fn() + Send + Sync>>>>>> =
            const { RefCell::new(Vec::new()) };
    }

    static JET_FFI_NEXT_CALLBACK_ID: AtomicU64 = AtomicU64::new(1);

    /// Guard proving that a foreign callback thread entered the Jet runtime.
    /// Generated trampolines create this guard before dispatch; a raw direct
    /// call without the guard is rejected and initiates registration shutdown.
    pub struct JetFfiRuntimeAttachment {
        previous: bool,
    }

    impl JetFfiRuntimeAttachment {
        pub fn enter() -> Self {
            let previous = JET_FFI_RUNTIME_ATTACHED.with(|attached| {
                let previous = attached.get();
                attached.set(true);
                previous
            });
            Self { previous }
        }

        pub fn is_attached() -> bool {
            JET_FFI_RUNTIME_ATTACHED.with(std::cell::Cell::get)
        }
    }
    impl Drop for JetFfiRuntimeAttachment {
        fn drop(&mut self) {
            JET_FFI_RUNTIME_ATTACHED.with(|attached| attached.set(self.previous));
        }
    }

    pub fn jet_ffi_with_callback_attachment<F, R>(run: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _attachment = JetFfiRuntimeAttachment::enter();
        run()
    }

    type JetFfiCallbackHandler<T: Send + Sync + 'static> = std::sync::Arc<
        dyn for<'a> Fn(JetFfiCallbackEvent<'a, T>) -> Result<(), String> + Send + Sync,
    >;

    struct JetFfiCallbackState<T: Send + Sync + 'static> {
        id: u64,
        contract: JetFfiCallbackContract,
        owner_thread: std::thread::ThreadId,
        phase: AtomicU8,
        in_flight: AtomicUsize,
        shutdown_started: AtomicU8,
        native_ack: AtomicU8,
        callback: Mutex<Option<JetFfiCallbackHandler<T>>>,
        native_stop: std::sync::Arc<dyn Fn() -> Result<(), String> + Send + Sync>,
        native_release: Mutex<Option<std::sync::Arc<dyn Fn() + Send + Sync>>>,
        callback_failure: Mutex<Option<String>>,
        native_failure: Mutex<Option<String>>,
        wait_lock: Mutex<()>,
        wake: Condvar,
    }

    impl<T: Send + Sync + 'static> JetFfiCallbackState<T> {
        fn new(
            contract: JetFfiCallbackContract,
            callback: JetFfiCallbackHandler<T>,
            native_stop: std::sync::Arc<dyn Fn() -> Result<(), String> + Send + Sync>,
            native_release: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
        ) -> Result<std::sync::Arc<Self>, String> {
            contract.validate(native_release.is_some())?;
            Ok(std::sync::Arc::new(Self {
                id: JET_FFI_NEXT_CALLBACK_ID.fetch_add(1, Ordering::Relaxed),
                contract,
                owner_thread: std::thread::current().id(),
                phase: AtomicU8::new(JET_FFI_CALLBACK_ACTIVE),
                in_flight: AtomicUsize::new(0),
                shutdown_started: AtomicU8::new(0),
                native_ack: AtomicU8::new(0),
                callback: Mutex::new(Some(callback)),
                native_stop,
                native_release: Mutex::new(native_release),
                callback_failure: Mutex::new(None),
                native_failure: Mutex::new(None),
                wait_lock: Mutex::new(()),
                wake: Condvar::new(),
            }))
        }

        fn request_stop(&self) {
            let _ = self.phase.compare_exchange(
                JET_FFI_CALLBACK_ACTIVE,
                JET_FFI_CALLBACK_STOPPING,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            self.wake.notify_all();
        }

        fn phase(&self) -> u8 {
            self.phase.load(Ordering::Acquire)
        }

        fn record_callback_failure(&self, message: String) {
            let mut failure = self
                .callback_failure
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if failure.is_none() {
                *failure = Some(message);
            }
            self.request_stop();
        }

        fn record_native_failure(&self, message: String) {
            let mut failure = self
                .native_failure
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if failure.is_none() {
                *failure = Some(message);
            }
            self.phase
                .store(JET_FFI_CALLBACK_QUARANTINED, Ordering::Release);
            self.wake.notify_all();
        }

        fn check_thread(&self) -> Result<(), JetFfiCallbackDispatch> {
            match self.contract.thread_affinity {
                JetFfiCallbackThreadAffinity::Any => Ok(()),
                JetFfiCallbackThreadAffinity::Main
                    if std::thread::current().id() == self.owner_thread => Ok(()),
                JetFfiCallbackThreadAffinity::AttachedForeign
                    if JetFfiRuntimeAttachment::is_attached() => Ok(()),
                JetFfiCallbackThreadAffinity::Main => Err(JetFfiCallbackDispatch::RejectedThread),
                JetFfiCallbackThreadAffinity::AttachedForeign => {
                    Err(JetFfiCallbackDispatch::RejectedThread)
                }
            }
        }

        fn enter(self: &std::sync::Arc<Self>) -> Result<JetFfiInFlight<T>, JetFfiCallbackDispatch> {
            if let Err(error) = self.check_thread() {
                self.record_callback_failure("callback entered without its vetted thread attachment".to_string());
                return Err(error);
            }
            match self.phase() {
                JET_FFI_CALLBACK_ACTIVE => {}
                JET_FFI_CALLBACK_QUARANTINED => {
                    return Err(JetFfiCallbackDispatch::RejectedQuarantined);
                }
                _ => return Err(JetFfiCallbackDispatch::RejectedStopped),
            }
            let reentrant = JET_FFI_CALLBACK_STACK.with(|stack| {
                stack.borrow().iter().any(|id| *id == self.id)
            });
            if reentrant && !self.contract.reentrant {
                self.record_callback_failure("reentrant callback rejected by registration contract".to_string());
                return Err(JetFfiCallbackDispatch::RejectedReentrant);
            }
            self.in_flight.fetch_add(1, Ordering::AcqRel);
            if self.phase() != JET_FFI_CALLBACK_ACTIVE {
                self.leave();
                return Err(JetFfiCallbackDispatch::RejectedStopped);
            }
            JET_FFI_CALLBACK_STACK.with(|stack| stack.borrow_mut().push(self.id));
            Ok(JetFfiInFlight {
                state: self.clone(),
                active: true,
                _marker: std::marker::PhantomData,
            })
        }

        fn leave(&self) {
            if self.in_flight.fetch_sub(1, Ordering::AcqRel) == 1 {
                self.wake.notify_all();
            }
        }

        fn dispatch(self: &std::sync::Arc<Self>, value: T) -> JetFfiCallbackDispatch {
            let guard = match self.enter() {
                Ok(guard) => guard,
                Err(error) => {
                    if matches!(
                        error,
                        JetFfiCallbackDispatch::RejectedThread
                            | JetFfiCallbackDispatch::RejectedReentrant
                    ) {
                        self.spawn_stop().detach();
                    }
                    return error;
                }
            };
            let stop_state = self.clone();
            let stop_slot: std::sync::Arc<
                std::sync::Mutex<Option<std::sync::Arc<dyn Fn() + Send + Sync>>>,
            > = std::sync::Arc::new(std::sync::Mutex::new(Some(
                std::sync::Arc::new(move || {
                    stop_state.request_stop();
                    stop_state.spawn_stop().detach();
                }),
            )));
            JET_FFI_CURRENT_EVENT_STOP.with(|stack| stack.borrow_mut().push(stop_slot));
            let callback = self
                .callback
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone();
            let result = match callback {
                Some(callback) => std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    callback(JetFfiCallbackEvent {
                        value: &value,
                        state: self.clone(),
                    })
                }))
                .map_err(|_| "callback panicked across the foreign boundary".to_string())
                .and_then(|result| result),
                None => Err("callback handler was released before admission completed".to_string()),
            };
            JET_FFI_CURRENT_EVENT_STOP.with(|stack| {
                let _ = stack.borrow_mut().pop();
            });
            JET_FFI_CALLBACK_STACK.with(|stack| {
                let _ = stack.borrow_mut().pop();
            });
            drop(guard);
            match result {
                Ok(()) => JetFfiCallbackDispatch::Accepted,
                Err(message) => {
                    self.record_callback_failure(message.clone());
                    self.spawn_stop().detach();
                    JetFfiCallbackDispatch::Failed(message)
                }
            }
        }

        fn spawn_stop(self: &std::sync::Arc<Self>) -> JetTask<JetFfiStopResult> {
            let state = self.clone();
            JetTask::spawn(move || state.finish_stop())
        }

        fn finish_stop(self: &std::sync::Arc<Self>) -> JetFfiStopResult {
            self.request_stop();
            let leader = self
                .shutdown_started
                .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok();
            if leader {
                let native_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    (self.native_stop)()
                }))
                .map_err(|_| "native shutdown hook panicked".to_string())
                .and_then(|result| result);
                if let Err(message) = native_result {
                    self.record_native_failure(message);
                    return self.stop_result();
                }
                self.native_ack.store(1, Ordering::Release);
                let mut wait = self
                    .wait_lock
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                while self.in_flight.load(Ordering::Acquire) != 0 {
                    wait = self
                        .wake
                        .wait(wait)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                }
                self.phase.store(JET_FFI_CALLBACK_STOPPED, Ordering::Release);
                let release = self
                    .native_release
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .take();
                let callback = self
                    .callback
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .take();
                drop(wait);
                drop(callback);
                if let Some(release) = release {
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| release()));
                }
                self.wake.notify_all();
            } else {
                let mut wait = self
                    .wait_lock
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                while matches!(self.phase(), JET_FFI_CALLBACK_STOPPING)
                    || self.in_flight.load(Ordering::Acquire) != 0
                {
                    wait = self
                        .wake
                        .wait(wait)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                }
            }
            self.stop_result()
        }

        fn stop_result(&self) -> JetFfiStopResult {
            if let Some(message) = self
                .native_failure
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
            {
                return JetFfiStopResult::Quarantined(message);
            }
            if let Some(message) = self
                .callback_failure
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
            {
                return JetFfiStopResult::CallbackFailed(message);
            }
            if self.phase() == JET_FFI_CALLBACK_STOPPED {
                JetFfiStopResult::Stopped
            } else {
                JetFfiStopResult::NativeBoundaryFailed(
                    "callback shutdown did not reach a terminal acknowledgement".to_string(),
                )
            }
        }
    }

    struct JetFfiInFlight<T: Send + Sync + 'static> {
        state: std::sync::Arc<JetFfiCallbackState<T>>,
        active: bool,
        _marker: std::marker::PhantomData<T>,
    }

    impl<T: Send + Sync + 'static> Drop for JetFfiInFlight<T> {
        fn drop(&mut self) {
            if self.active {
                self.active = false;
                self.state.leave();
            }
        }
    }

    pub struct JetFfiCallbackEvent<'a, T: Send + Sync + 'static> {
        value: &'a T,
        state: std::sync::Arc<JetFfiCallbackState<T>>,
    }

    impl<'a, T: Send + Sync + 'static> JetFfiCallbackEvent<'a, T> {
        pub fn value(&self) -> &'a T {
            self.value
        }

        /// Nonblocking self-stop. It only closes admission and schedules the
        /// native shutdown worker; it never waits for the current invocation.
        pub fn stop(&self) {
            self.state.request_stop();
            self.state.spawn_stop().detach();
        }

        pub fn callback_identity(&self) -> &str {
            &self.state.contract.callback_identity
        }

        pub fn plan_digest(&self) -> &str {
            &self.state.contract.plan_digest
        }
    }
 
    /// Owned event value passed to a generated managed callback target. The
    /// value is copied out of the native invocation before user code runs;
    /// `stop()` still addresses the active registration through the vetted
    /// callback-entry TLS slot.
    #[derive(Clone, Debug)]
    pub struct JetFfiCallbackEventValue<T: Send + Sync + 'static> {
        pub value: T,
    }

    impl<T: Send + Sync + 'static> JetFfiCallbackEventValue<T> {
        pub fn new(value: T) -> Self {
            Self { value }
        }

        pub fn value(&self) -> &T {
            &self.value
        }

        pub fn stop(&self) {
            // The raw event-stop operation is intentionally nonblocking. It
            // requests native shutdown but never joins from callback code.
            unsafe {
                let _ = jet_ffi_callback_event_stop();
            }
        }
    }

    pub struct JetFfiCallbackRegistration<T: Send + Sync + 'static> {
        state: std::sync::Arc<JetFfiCallbackState<T>>,
        owner_consumed: bool,
    }

    impl<T: Send + Sync + 'static> JetFfiCallbackRegistration<T> {
        pub fn new(
            contract: JetFfiCallbackContract,
            callback: JetFfiCallbackHandler<T>,
            native_stop: std::sync::Arc<dyn Fn() -> Result<(), String> + Send + Sync>,
            native_release: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
        ) -> Result<Self, String> {
            Ok(Self {
                state: JetFfiCallbackState::new(
                    contract,
                    callback,
                    native_stop,
                    native_release,
                )?,
                owner_consumed: false,
            })
        }

        pub fn dispatch(&self, value: T) -> JetFfiCallbackDispatch {
            self.state.dispatch(value)
        }

        pub fn stop(&self) {
            self.state.request_stop();
            self.state.spawn_stop().detach();
        }

        /// Consume the registration owner. The returned task completes only
        /// after native shutdown ACK and all admitted callbacks finish.
        pub fn unsubscribe(mut self) -> JetTask<JetFfiStopResult> {
            self.owner_consumed = true;
            self.state.request_stop();
            self.state.spawn_stop()
        }

        pub fn phase(&self) -> JetFfiCallbackPhase {
            JetFfiCallbackPhase::from_raw(self.state.phase())
        }

        pub fn contract(&self) -> &JetFfiCallbackContract {
            &self.state.contract
        }
    }

    impl<T: Send + Sync + 'static> Drop for JetFfiCallbackRegistration<T> {
        fn drop(&mut self) {
            if !self.owner_consumed {
                // A generated owner must normally consume `unsubscribe`; a
                // dropped owner still closes admission so native code cannot
                // continue calling a released closure. The detached task keeps
                // state and native context retained until ACK or quarantine.
                self.state.request_stop();
                self.state.spawn_stop().detach();
            }
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum JetFfiCallbackPhase {
        Active,
        Stopping,
        Stopped,
        Quarantined,
    }

    impl JetFfiCallbackPhase {
        fn from_raw(raw: u8) -> Self {
            match raw {
                JET_FFI_CALLBACK_ACTIVE => Self::Active,
                JET_FFI_CALLBACK_STOPPING => Self::Stopping,
                JET_FFI_CALLBACK_STOPPED => Self::Stopped,
                _ => Self::Quarantined,
            }
        }
    }

    // ── C ABI glue ──────────────────────────────────────────────────────────
    // The C side receives one opaque handle. It may invoke concurrently, ask
    // for a nonblocking stop, and consume the handle through unsubscribe. The
    // handle remains allocated in the stop task until ACK/quiescence, closing
    // the late-callback use-after-free window.
    pub type JetFfiRawInvoke = unsafe extern "C" fn(*mut c_void, i64);
    pub type JetFfiRawStop = unsafe extern "C" fn(*mut c_void) -> i32;
    pub type JetFfiRawRelease = unsafe extern "C" fn(*mut c_void);

    struct JetFfiRawRegistration {
        registration: Mutex<Option<JetFfiCallbackRegistration<i64>>>,
        event_stop: std::sync::Arc<Mutex<Option<std::sync::Arc<dyn Fn() + Send + Sync>>>>,
    }
 
    /// Source-facing owner for a native callback registration. Moving this
    /// handle into `unsubscribe` consumes the only owner; dropping it starts
    /// the same asynchronous shutdown path and keeps the raw allocation alive
    /// until the stop task drains.
    #[repr(transparent)]
    #[derive(Debug)]
    pub struct JetFfiCallbackRegistrationHandle {
        raw: *mut c_void,
    }

    // The pointed-to raw registration is synchronized internally. The handle
    // may therefore cross a task boundary even though its owner operation is
    // consuming.
    unsafe impl Send for JetFfiCallbackRegistrationHandle {}
    unsafe impl Sync for JetFfiCallbackRegistrationHandle {}

    impl JetFfiCallbackRegistrationHandle {
        pub fn from_raw(raw: *mut c_void) -> Self {
            assert!(!raw.is_null(), "managed callback registration returned a null handle");
            Self { raw }
        }

        pub fn as_raw(&self) -> *mut c_void {
            self.raw
        }

        pub fn into_raw(mut self) -> *mut c_void {
            let raw = self.raw;
            self.raw = std::ptr::null_mut();
            raw
        }
    }

    impl Drop for JetFfiCallbackRegistrationHandle {
        fn drop(&mut self) {
            if self.raw.is_null() {
                return;
            }
            // Drop is not a join point. The detached task owns the raw
            // registration box until native ACK and in-flight drain.
            unsafe {
                let stop_task = jet_ffi_callback_registration_unsubscribe(self.raw);
                if !stop_task.is_null() {
                    jet_ffi_callback_stop_task_detach(stop_task);
                }
            }
            self.raw = std::ptr::null_mut();
        }
    }

    /// Build a managed registration around a generated native start function.
    /// The start closure receives the callback trampoline and the freshly
    /// allocated registration context. Native code must retain that context
    /// until its stop function returns the shutdown acknowledgement.
    pub fn jet_ffi_callback_registration_start_i64<Start, Stop, Release>(
        callback: Option<JetFfiRawInvoke>,
        native_start: Start,
        native_stop: Stop,
        native_release: Release,
        handler: JetFfiCallbackHandler<i64>,
        callback_identity: &str,
        plan_digest: &str,
    ) -> *mut c_void
    where
        Start: Fn(Option<JetFfiRawInvoke>, *mut c_void) -> *mut c_void + Send + Sync + 'static,
        Stop: Fn(*mut c_void) -> i32 + Send + Sync + 'static,
        Release: Fn(*mut c_void) + Send + Sync + 'static,
    {
        let native_token_slot = std::sync::Arc::new(std::sync::Mutex::new(None::<usize>));
        // Native start is allowed to deliver a callback before it returns its
        // token. A stop requested by that synchronous callback must wait for
        // the start call to publish the token instead of racing it into a
        // false "no native token" quarantine.
        let start_gate = std::sync::Arc::new((std::sync::Mutex::new(false), Condvar::new()));
        let stop_gate = start_gate.clone();
        let stop_context = native_token_slot.clone();
        let native_stop: std::sync::Arc<dyn Fn() -> Result<(), String> + Send + Sync> =
            std::sync::Arc::new(move || {
                let (complete, wake) = &*stop_gate;
                let mut complete = complete
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                while !*complete {
                    complete = wake
                        .wait(complete)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                }
                drop(complete);
                let token = *stop_context
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let Some(token) = token else {
                    return Err("managed callback has no native token".to_string());
                };
                let status = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    native_stop(token as *mut c_void)
                }))
                .map_err(|_| "native callback shutdown panicked".to_string())?;
                if status == 0 {
                    Ok(())
                } else {
                    Err(format!("native callback shutdown returned status {status}"))
                }
            });
        let release_context = native_token_slot.clone();
        let native_release: std::sync::Arc<dyn Fn() + Send + Sync> =
            std::sync::Arc::new(move || {
                let token = *release_context
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if let Some(token) = token {
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        native_release(token as *mut c_void)
                    }));
                }
            });
        if callback_identity.trim().is_empty()
            || plan_digest.trim().is_empty()
            || plan_digest != jet_ffi_callback_plan_digest(callback_identity)
        {
            return std::ptr::null_mut();
        }
        let contract =
            JetFfiCallbackContract::managed(callback_identity.to_string(), plan_digest.to_string());
        let registration = match JetFfiCallbackRegistration::new(
            contract,
            handler,
            native_stop,
            Some(native_release),
        ) {
            Ok(registration) => registration,
            Err(_) => return std::ptr::null_mut(),
        };
        let event_stop: std::sync::Arc<
            std::sync::Mutex<Option<std::sync::Arc<dyn Fn() + Send + Sync>>>,
        > = std::sync::Arc::new(std::sync::Mutex::new(None));
        let state = registration.state.clone();
        *event_stop
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) =
            Some(std::sync::Arc::new(move || {
                state.request_stop();
                state.spawn_stop().detach();
            }));
        let raw = Box::new(JetFfiRawRegistration {
            registration: Mutex::new(Some(registration)),
            event_stop,
        });
        let raw = Box::into_raw(raw);
        let started = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            native_start(callback, raw.cast())
        }))
        .ok()
        .filter(|token| !token.is_null())
        .map(|token| {
            *native_token_slot
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(token as usize);
            true
        })
        .unwrap_or(false);
        let (complete, wake) = &*start_gate;
        *complete
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
        wake.notify_all();
        if !started {
            unsafe {
                let stop_task = jet_ffi_callback_registration_unsubscribe(raw.cast());
                if !stop_task.is_null() {
                    jet_ffi_callback_stop_task_detach(stop_task);
                }
            }
            return std::ptr::null_mut();
        }
        raw.cast()
    }
    struct JetFfiRawStopTask {
        task: Option<JetTask<JetFfiStopResult>>,
        // Keep the opaque registration address valid until the caller joins
        // the completion task. Native code can therefore deliver a late call;
        // it sees the consumed slot and executes no user callback.
        registration: Option<Box<JetFfiRawRegistration>>,
        // A failed ACK is quarantined, not freed. This Arc keeps the state,
        // failure evidence, and retained captures alive with the leaked holder.
        retained_state: Option<std::sync::Arc<JetFfiCallbackState<i64>>>,
    }

    fn raw_text(ptr: *const c_char) -> String {
        if ptr.is_null() {
            return String::new();
        }
        // SAFETY: the generated facade passes NUL-terminated identity strings;
        // invalid UTF-8 is retained lossily rather than invoking undefined
        // behavior or inventing a replacement identity.
        unsafe { CStr::from_ptr(ptr).to_string_lossy().into_owned() }
    }

    fn raw_status(outcome: JetFfiCallbackDispatch) -> i32 {
        match outcome {
            JetFfiCallbackDispatch::Accepted => 0,
            JetFfiCallbackDispatch::RejectedStopped => 1,
            JetFfiCallbackDispatch::RejectedReentrant => 2,
            JetFfiCallbackDispatch::RejectedThread => 3,
            JetFfiCallbackDispatch::RejectedQuarantined => 4,
            JetFfiCallbackDispatch::Failed(_) => 5,
        }
    }

    fn raw_stop_status(result: Result<JetFfiStopResult, JetTaskFailure>) -> i32 {
        match result {
            Ok(JetFfiStopResult::Stopped) => 0,
            Ok(JetFfiStopResult::CallbackFailed(_)) => 2,
            Ok(JetFfiStopResult::NativeBoundaryFailed(_)) => 3,
            Ok(JetFfiStopResult::Quarantined(_)) => 4,
            Err(JetTaskFailure::Cancelled) => 5,
            Err(JetTaskFailure::DeadlineBlown) => 6,
            Err(JetTaskFailure::Panicked(_)) => 7,
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn jet_ffi_callback_event_stop() -> i32 {
        let stop = JET_FFI_CURRENT_EVENT_STOP.with(|stack| stack.borrow().last().cloned());
        let Some(stop) = stop else {
            return -1;
        };
        let Some(stop) = stop.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone() else {
            return -1;
        };
        stop();
        0
    }
 
    /// Unit-returning route used by `event.stop()` in generated callbacks.
    /// It deliberately discards the status because admission closure is
    /// asynchronous; the consuming owner reports native failures on join.
    pub fn jet_ffi_callback_event_stop_unit() {
        unsafe {
            let _ = jet_ffi_callback_event_stop();
        }
    }

    #[no_mangle]
    pub unsafe extern "C" fn jet_ffi_callback_registration_new_i64(
        invoke: Option<JetFfiRawInvoke>,
        stop: Option<JetFfiRawStop>,
        release: Option<JetFfiRawRelease>,
        ctx: *mut c_void,
        callback_identity: *const c_char,
        plan_digest: *const c_char,
        flags: u32,
    ) -> *mut c_void {
        let (Some(invoke), Some(stop), Some(release)) = (invoke, stop, release) else {
            return std::ptr::null_mut();
        };
        if flags & JET_FFI_CALLBACK_REQUIRED_FLAGS != JET_FFI_CALLBACK_REQUIRED_FLAGS
            || flags & !(JET_FFI_CALLBACK_REQUIRED_FLAGS | JET_FFI_CALLBACK_FLAG_REENTRANT) != 0
        {
            return std::ptr::null_mut();
        }
        let callback_identity = raw_text(callback_identity);
        let plan_digest = raw_text(plan_digest);
        if callback_identity.trim().is_empty()
            || plan_digest.trim().is_empty()
            || plan_digest != jet_ffi_callback_plan_digest(&callback_identity)
        {
            return std::ptr::null_mut();
        }
        let mut contract = JetFfiCallbackContract::managed(callback_identity, plan_digest);
        contract.reentrant = flags & JET_FFI_CALLBACK_FLAG_REENTRANT != 0;
        let event_stop: std::sync::Arc<
            Mutex<Option<std::sync::Arc<dyn Fn() + Send + Sync>>>,
        > = std::sync::Arc::new(Mutex::new(None));
        let callback_ctx = ctx as usize;
        let stop_slot = event_stop.clone();
        let callback: JetFfiCallbackHandler<i64> = std::sync::Arc::new(move |event| {
            JET_FFI_CURRENT_EVENT_STOP.with(|stack| stack.borrow_mut().push(stop_slot.clone()));
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                invoke(callback_ctx as *mut c_void, *event.value());
            }))
            .map_err(|_| "native callback trampoline panicked".to_string());
            JET_FFI_CURRENT_EVENT_STOP.with(|stack| {
                let _ = stack.borrow_mut().pop();
            });
            result
        });
        let stop_ctx = ctx as usize;
        let native_stop: std::sync::Arc<dyn Fn() -> Result<(), String> + Send + Sync> =
            std::sync::Arc::new(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                    stop(stop_ctx as *mut c_void)
                }))
                .map_err(|_| "native callback shutdown panicked".to_string())?;
                if result == 0 {
                    Ok(())
                } else {
                    Err(format!("native callback shutdown returned status {result}"))
                }
            });
        let release_ctx = ctx as usize;
        let native_release: std::sync::Arc<dyn Fn() + Send + Sync> = std::sync::Arc::new(move || {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                release(release_ctx as *mut c_void)
            }));
        });
        let registration = match JetFfiCallbackRegistration::new(
            contract,
            callback,
            native_stop,
            Some(native_release),
        ) {
            Ok(registration) => registration,
            Err(_) => return std::ptr::null_mut(),
        };
        let state = registration.state.clone();
        *event_stop
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) =
            Some(std::sync::Arc::new(move || {
                state.request_stop();
                state.spawn_stop().detach();
            }));
        let raw = Box::new(JetFfiRawRegistration {
            registration: Mutex::new(Some(registration)),
            event_stop,
        });
        Box::into_raw(raw).cast()
    }

    pub unsafe extern "C" fn jet_ffi_callback_registration_invoke_i64(
        handle: *mut c_void,
        value: i64,
    ) -> i32 {
        if handle.is_null() {
            return 1;
        }
        let raw = &*(handle.cast::<JetFfiRawRegistration>());
        let state = raw
            .registration
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .map(|registration| registration.state.clone());
        let Some(state) = state else {
            return 1;
        };
        // Do not hold the owner mutex while dispatching: a reentrant callback
        // may enter this trampoline again before the outer invocation leaves.
        jet_ffi_with_callback_attachment(|| raw_status(state.dispatch(value)))
    }

    #[no_mangle]
    pub unsafe extern "C" fn jet_ffi_callback_registration_stop(handle: *mut c_void) -> i32 {
        if handle.is_null() {
            return 1;
        }
        let raw = &*(handle.cast::<JetFfiRawRegistration>());
        let registration = raw
            .registration
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(registration) = registration.as_ref() else {
            return 1;
        };
        registration.stop();
        0
    }

    #[no_mangle]
    pub unsafe extern "C" fn jet_ffi_callback_registration_unsubscribe(
        handle: *mut c_void,
    ) -> *mut c_void {
        if handle.is_null() {
            return std::ptr::null_mut();
        }
        let raw = Box::from_raw(handle.cast::<JetFfiRawRegistration>());
        let registration = raw
            .registration
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        let Some(registration) = registration else {
            std::mem::forget(raw);
            return std::ptr::null_mut();
        };
        let retained_state = registration.state.clone();
        let task = registration.unsubscribe();
        let stop_task = Box::new(JetFfiRawStopTask {
            task: Some(task),
            registration: Some(raw),
            retained_state: Some(retained_state),
        });
        Box::into_raw(stop_task).cast()
    }

    #[no_mangle]
    pub unsafe extern "C" fn jet_ffi_callback_stop_task_join(handle: *mut c_void) -> i32 {
        if handle.is_null() {
            return 7;
        }
        let mut task = Box::from_raw(handle.cast::<JetFfiRawStopTask>());
        let result = task
            .task
            .take()
            .map(|task| task.join())
            .unwrap_or(Err(JetTaskFailure::Panicked(
                "callback stop task was already joined".to_string(),
            )));
        let status = raw_stop_status(result);
        let stopped = task
            .retained_state
            .as_ref()
            .is_some_and(|state| state.phase() == JET_FFI_CALLBACK_STOPPED);
        if status != 0 && !stopped {
            // A failed native ACK or callback drain is a quarantine outcome.
            // Retain the wrapper and its native context instead of dropping
            // memory while foreign code may still hold the opaque handle.
            std::mem::forget(task);
        }
        status
    }

    #[no_mangle]
    pub unsafe extern "C" fn jet_ffi_callback_registration_phase(handle: *mut c_void) -> i32 {
        if handle.is_null() {
            return JET_FFI_CALLBACK_QUARANTINED as i32;
        }
        let raw = &*(handle.cast::<JetFfiRawRegistration>());
        let registration = raw
            .registration
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        registration
            .as_ref()
            .map(|registration| registration.phase() as i32)
            .unwrap_or(JET_FFI_CALLBACK_STOPPED as i32)
    }
 
    /// Detach a stop task while preserving its registration on any failed
    /// shutdown. Successful completion drops the holder after ACK and drain;
    /// failure intentionally leaks the holder as a quarantined live state.
    #[no_mangle]
    pub unsafe extern "C" fn jet_ffi_callback_stop_task_detach(handle: *mut c_void) {
        if handle.is_null() {
            return;
        }
        let mut holder = Box::from_raw(handle.cast::<JetFfiRawStopTask>());
        JetTask::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                holder
                    .task
                    .take()
                    .map(|task| task.join())
                    .unwrap_or(Err(JetTaskFailure::Panicked(
                        "callback stop task was already joined".to_string(),
                    )))
            }));
            let status = result.map(raw_stop_status).unwrap_or(7);
            let stopped = holder
                .retained_state
                .as_ref()
                .is_some_and(|state| state.phase() == JET_FFI_CALLBACK_STOPPED);
            if status == 0 || stopped {
                drop(holder);
            } else {
                std::mem::forget(holder);
            }
        })
        .detach();
    }

    /// Consuming source-facing unsubscribe. The task's `Ok` value is produced
    /// only after native ACK, callback drain, and capture/trampoline release.
    pub fn jet_ffi_callback_registration_unsubscribe_result(
        handle: *mut c_void,
    ) -> JetTask<JetOutcome<(), String>> {
        let stop_task = unsafe { jet_ffi_callback_registration_unsubscribe(handle) };
        if stop_task.is_null() {
            return JetTask::spawn(|| Err("callback registration was already consumed".to_string()));
        }
        let stop_task_addr = stop_task as usize;
        JetTask::spawn(move || {
            let status = unsafe { jet_ffi_callback_stop_task_join(stop_task_addr as *mut c_void) };
            if status == 0 {
                Ok(())
            } else {
                Err(format!("callback unsubscribe failed with status {status}"))
            }
        })
    }
