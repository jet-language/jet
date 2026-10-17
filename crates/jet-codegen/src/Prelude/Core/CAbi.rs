// D-EXEC1: the C-ABI export surface of the one compiled runtime.
//
// The Jet-native backend (Compiler/JetBackend, contract in
// Docs/research/jet-backend-design-2026-10-01.md, "Calls into the compiled
// runtime") calls the runtime by symbol with the System V convention. Every
// export below is a carrier adapter over the Prelude function AOT already
// calls: no export decides meaning of its own (I9).
//
// Carrier rules, one machine word each:
// - `Int` is the exact-Int carrier word (inline, or a tagged big-integer
//   owner). Kernels that take native `i64` are adapted the way generated
//   Rust adapts them (`native_int_input` / `native_int_result`).
// - `String` is a pointer to `JetCText`, the C-layout {ptr, len, cap}
//   UTF-8 carrier. Drop and builder finish consume it; other parameters borrow
//   it. Its header and buffer use the runtime's shared allocation domain.
// - `Bool` parameters are a word holding 0 or 1; `Bool` results are returned
//   in the low byte.
// - A cursor handle is an owned `Box<JetLoopRangeCursor>`.
//
// Generated code has no unwind tables, so no unwind may leave an export.
// Intentional stops return through the canonical Task or process Root rail;
// the caller's generated Poll runs lexical cleanup before observing a result.
#[cfg(not(target_arch = "wasm32"))]
mod jet_c_abi {
    #![allow(non_snake_case)]
    use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};

    /// Stable text header shared with Jet runtime layers. `ptr` is non-null,
    /// `len <= cap`, and the first `len` bytes are initialized valid UTF-8.
    /// Owning headers/buffers belong to jet_rt_alloc/jet_rt_free, not Rust's
    /// heap. When cap is 0, ptr is 1 and is never freed. Field projections may
    /// lend a stack header over Rust bytes; that header is read/clone-only.
    #[repr(C)]
    pub(crate) struct JetCText {
        pub(crate) ptr: *mut u8,
        pub(crate) len: usize,
        pub(crate) cap: usize,
    }

    pub(crate) type JetCString = *mut JetCText;

    #[cfg(jet_native_pack)]
    unsafe extern "C" {
        fn jet_rt_stop_domain() -> i64;
        fn jet_rt_root_begin(save: *mut i64) -> i64;
        fn jet_rt_root_publish_rendered(bytes: *const u8, len: usize, code: i64) -> bool;
        fn jet_rt_root_take_stop(control: i64, code: *mut i64) -> i64;
        fn jet_rt_root_leave(control: i64, save: *const i64);
        fn jet_rt_session_status() -> i64;
        fn jet_rt_session_cleanup_enter(delivered_at_entry: i64) -> i64;
        fn jet_rt_session_cleanup_leave(saved_flags: i64);
        fn jet_rt_stop_take_message(record: i64) -> JetCString;
        fn jet_rt_stop_drop(record: i64);
        fn jet_rt_control_publish_stop(kind: i64, jct: i64, owner: i64, start: i64, end: i64, bytes: *const u8, len: usize, form: i64) -> bool;
    }

    /// Read the Jet-owned domain; never synthesize a Task or inspect its flags.
    #[cfg(jet_native_pack)]
    pub(crate) fn stop_domain() -> i64 {
        // SAFETY: the native pack initializer attaches this thread's TCB.
        unsafe { jet_rt_stop_domain() }
    }

    /// One actual process-finalizer scope. Its zero token is also owned and
    /// must be restored; Rust never reads or changes the Control's flags.
    #[cfg(jet_native_pack)]
    pub(crate) struct NativeCleanupScope(i64);

    #[cfg(jet_native_pack)]
    impl NativeCleanupScope {
        pub(crate) fn enter() -> Self {
            // SAFETY: the raw status is sampled at this real cleanup entry.
            // Enter borrows the current control and returns its opaque token.
            unsafe { Self(jet_rt_session_cleanup_enter(jet_rt_session_status())) }
        }
    }

    #[cfg(jet_native_pack)]
    impl Drop for NativeCleanupScope {
        fn drop(&mut self) {
            // SAFETY: nested Root/control scopes restore their parent before
            // this lexical scope ends. Restore this owned token exactly once.
            unsafe { jet_rt_session_cleanup_leave(self.0) };
        }
    }

    /// Publish only typed program transfers. Unknown payloads keep the fatal
    /// host boundary; no manufactured message or success value is observable.
    #[cfg(jet_native_pack)]
    fn native_record_unwind(
        payload: Box<dyn std::any::Any + Send>,
    ) -> Result<(), Box<dyn std::any::Any + Send>> {
        if super::jet_native_comptime_active() {
            return Err(payload);
        }
        let accepted = match stop_domain() {
            2 => {
                let Some(panic) = payload.downcast_ref::<super::JetNativeTaskPanic>() else {
                    return Err(payload);
                };
                // SAFETY: checked publication copies these borrowed bytes into
                // the current Jet Task's owned raw-reason record.
                unsafe {
                    jet_rt_control_publish_stop(
                        5, -1, -1, -1, -1, panic.0.as_ptr(), panic.0.len(), 0,
                    )
                }
            }
            1 => {
                let (report, code) =
                    if let Some(report) = payload.downcast_ref::<super::JetRuntimeDiagnostic>() {
                        (Some(report.rendered.as_str()), report.exit_code)
                    } else if let Some(report) = payload.downcast_ref::<super::JetRenderedRuntimeStop>() {
                        (Some(report.rendered.as_str()), report.exit_code)
                    } else if let Some(exit) = payload.downcast_ref::<super::JetExplicitExit>() {
                        (None, exit.code)
                    } else if payload.is::<super::JetRuntimeExit>() {
                        (None, 70)
                    } else if let Some(report) = super::jet_runtime_foreign_stop_report(&*payload) {
                        // SAFETY: Root publication copies the rendered frame
                        // before this Rust-owned report is dropped.
                        return if unsafe {
                            jet_rt_root_publish_rendered(
                                report.rendered.as_ptr(), report.rendered.len(),
                                i64::from(report.exit_code),
                            )
                        } { Ok(()) } else { Err(payload) };
                    } else {
                        return Err(payload);
                    };
                let (bytes, len) = report
                    .map_or((std::ptr::null(), 0), |report| (report.as_ptr(), report.len()));
                // SAFETY: Root publication copies borrowed bytes, preserving
                // requested code and an owned non-null token even for exit0.
                unsafe { jet_rt_root_publish_rendered(bytes, len, i64::from(code)) }
            }
            _ => return Err(payload),
        };
        if accepted { Ok(()) } else { Err(payload) }
    }

    /// The word is unused after a typed transfer: MIR requires a Poll before
    /// any read or drop of a can-stop call result. This is not a constructed
    /// Jet value, nor a default value returned to user code.
    pub(crate) trait StoppedCarrier {
        fn stopped() -> Self;
    }
    macro_rules! stopped_word {
        ($($ty:ty => $zero:expr),* $(,)?) => {$(
            impl StoppedCarrier for $ty {
                fn stopped() -> Self { $zero }
            }
        )*};
    }
    stopped_word! {
        () => (), bool => false,
        i8 => 0, i16 => 0, i32 => 0, i64 => 0, isize => 0,
        u8 => 0, u16 => 0, u32 => 0, u64 => 0, usize => 0,
        f32 => 0.0, f64 => 0.0,
    }
    impl<T> StoppedCarrier for *mut T {
        fn stopped() -> Self { std::ptr::null_mut() }
    }
    impl<T> StoppedCarrier for *const T {
        fn stopped() -> Self { std::ptr::null() }
    }

    /// No Rust unwind may leave an export. The canonical native domain owns
    /// program transfers; the existing scheduler owns seed and comptime
    /// cancellation/deadline transfers. Unknown host payloads remain fatal.
    #[inline(always)]
    pub(crate) fn guard<T: StoppedCarrier>(run: impl FnOnce() -> T) -> T {
        match super::jet_scheduler_catch_foreign_boundary(run) {
            Ok(value) => value,
            Err(payload) => {
                #[cfg(jet_native_pack)]
                let payload = match native_record_unwind(payload) {
                    Ok(()) => return T::stopped(),
                    Err(payload) => payload,
                };
                match super::jet_scheduler_native_record_unwind(payload) {
                    Ok(()) => T::stopped(),
                    Err(payload) if super::jet_native_comptime_active() => {
                        let message = payload.downcast_ref::<String>().map(String::as_str)
                            .or_else(|| payload.downcast_ref::<&str>().copied())
                            .unwrap_or("native compiler runtime carried an unknown panic payload");
                        super::jet_native_comptime_fail(4, message)
                    }
                    Err(payload) => stop(payload),
                }
            }
        }
    }

    #[cold]
    fn stop(payload: Box<dyn std::any::Any + Send>) -> ! {
        // The boundary renders a runtime report and exits; any other payload
        // is a host fault, which ends like a panic leaving an AOT `main` (the
        // panic hook has already printed it).
        let _ = catch_unwind(AssertUnwindSafe(move || {
            super::jet_runtime_boundary(move || -> () { resume_unwind(payload) })
        }));
        super::jet_runtime_finalized_exit(101)
    }

    fn bytes<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
        if len == 0 {
            return &[];
        }
        // SAFETY: the caller passes `len` initialized bytes at `ptr` (static
        // data emitted by the backend) that outlive this call.
        unsafe { std::slice::from_raw_parts(ptr, len) }
    }

    fn text(ptr: *const u8, len: usize) -> std::borrow::Cow<'static, str> {
        String::from_utf8_lossy(bytes(ptr, len))
    }

    pub(crate) fn text_handle(value: &str) -> JetCString {
        let len = value.len();
        // SAFETY: a nonempty UTF-8 byte buffer has the valid (len, 1) layout;
        // the runtime pack's init array initializes the shared Jet heap first.
        let ptr = if len == 0 { std::ptr::NonNull::<u8>::dangling().as_ptr() } else { unsafe { jet_rt_alloc(len, 1) } };
        // SAFETY: the header has its exact nonzero C-layout size/alignment.
        let carrier = unsafe { jet_rt_alloc(std::mem::size_of::<JetCText>(), std::mem::align_of::<JetCText>()) } as JetCString;
        // SAFETY: both fresh allocations have exactly the requested sizes;
        // the input is valid UTF-8 and disjoint from the new buffer.
        unsafe {
            std::ptr::copy_nonoverlapping(value.as_ptr(), ptr, len);
            carrier.write(JetCText { ptr, len, cap: len });
        }
        carrier
    }
    pub(crate) fn handle(value: String) -> JetCString {
        text_handle(&value)
    }

    pub(crate) fn view<'a>(value: JetCString) -> &'a str {
        // SAFETY: the caller lends a live carrier and its valid UTF-8 buffer
        // for this call; neither is mutated while the view is borrowed.
        unsafe {
            let value = &*value;
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(value.ptr, value.len))
        }
    }

    /// Kernels whose signature requires `&String` get a Rust-owned copy.
    /// A carrier buffer must never be reallocated or freed by Rust String.
    pub(crate) fn borrowed(value: JetCString) -> String {
        view(value).to_owned()
    }

    pub(crate) fn release(value: JetCString) {
        // SAFETY: the caller transfers one live carrier and its buffer, both
        // allocated by jet_rt_alloc with the sizes recorded in the header.
        unsafe {
            if (*value).cap != 0 { jet_rt_free((*value).ptr, (*value).cap, 1); }
            jet_rt_free(value.cast(), std::mem::size_of::<JetCText>(), std::mem::align_of::<JetCText>());
        }
    }

    pub(crate) fn take(value: JetCString) -> String {
        let text = view(value).to_owned();
        release(value);
        text
    }

    fn line_of(line: i64) -> u32 {
        u32::try_from(line).unwrap_or(0)
    }

    /// `native_int_input`: an exact Int a native kernel receives as `i64`.
    pub(crate) fn native(value: i64) -> i64 {
        crate::jet_std::jet_int_to_i64(value).unwrap_or_else(|| {
            super::jet_arithmetic_stop("<core.prelude>", 0, "native Int argument exceeds host range")
        })
    }

    // Process entry.

    #[cfg(not(jet_native_pack))]
    #[no_mangle]
    pub extern "C" fn jet_rt_main(entry: extern "C" fn()) -> i32 {
        guard(|| {
            super::jet_std_env_init();
            #[cfg(jet_native_pack)]
            { native_main(entry) }
            #[cfg(not(jet_native_pack))]
            {
                super::jet_runtime_boundary(|| entry());
                0
            }
        })
    }

    #[cfg(jet_native_pack)]
    fn native_main(entry: extern "C" fn()) -> i32 {
        // Four borrowed TCB words, with their exact native 32B/8-byte layout.
        let mut save = [0_i64; 4];
        // SAFETY: begin owns the fresh Root and writes only this live save slot.
        let control = unsafe { jet_rt_root_begin(save.as_mut_ptr()) };
        entry();

        let mut parked_exit = None;
        guard(|| {
            let (parked, stop) = super::jet_runtime_finalize_process(false);
            parked_exit = parked;
            if let Some(payload) = stop {
                // A Rust frame catches this before the C ABI can return; native
                // machine frames have already returned through lexical cleanup.
                resume_unwind(payload);
            }
        });

        let mut code = 0_i64;
        // SAFETY: all owed finalizers have completed. Take transfers ownership
        // and writes code only for a nonzero record, including explicit exit0.
        let record = unsafe { jet_rt_root_take_stop(control, &mut code) };
        // SAFETY: leave consumes this Root and restores the paired four words;
        // the taken record and message outlive the control independently.
        unsafe { jet_rt_root_leave(control, save.as_ptr()) };
        if record != 0 {
            // SAFETY: take_message transfers the owned E4 carrier; drop consumes
            // only the remaining record. No raw Stop layout or policy is read.
            let message = unsafe { jet_rt_stop_take_message(record) };
            unsafe { jet_rt_stop_drop(record) };
            super::jet_runtime_process_report(
                (!message.is_null()).then(|| view(message)),
            );
            if !message.is_null() { release(message); }
            // The canonical Root producer supplies the requested native i32
            // exit carrier. Release all owned bytes before process::exit.
            super::jet_runtime_terminal_tail(code as i32);
        }
        if let Some(code) = parked_exit {
            super::jet_runtime_finalized_exit(code);
        }
        super::jet_runtime_flush_stdout();
        0
    }

    // A native comptime request (unified host/stop contract, #4720). begin
    // enters the request's own task control on the calling thread; a budget
    // operation that runs out publishes the request's first budget stop on the
    // current task's control and returns, so its MIR Propagate takes the stop
    // edge. The host reports that record (jet_rt_comptime_stop_write) or the
    // fatal one comptime_fatal writes.
    static COMPTIME_STOP: std::sync::Mutex<Option<super::JetStopRecord>> = std::sync::Mutex::new(None);
    static COMPTIME_STOPPED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    static EXECUTION_COMPTIME: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
    thread_local! {
        static COMPTIME_SCOPE: std::cell::RefCell<Option<super::JetNativeTaskControlScope>> = const { std::cell::RefCell::new(None) };
    }

    fn comptime_site(kind: super::JetStopKind, status: u32, message: String) -> super::JetStopRecord {
        let (_, owner, start, end) = super::jet_native_comptime_frame();
        super::JetStopRecord {
            kind,
            jct_status: Some(status),
            owner: EXECUTION_COMPTIME.load(std::sync::atomic::Ordering::Acquire).then_some(owner),
            start: EXECUTION_COMPTIME.load(std::sync::atomic::Ordering::Acquire).then_some(start),
            end: EXECUTION_COMPTIME.load(std::sync::atomic::Ordering::Acquire).then_some(end),
            message,
        }
    }

    // Interim native comptime budget publication; dies when comptime ports.
    // The native pack uses Jet's current control, never a second Rust rail.
    fn comptime_publish_stop(record: super::JetStopRecord) {
        #[cfg(not(jet_native_pack))]
        { super::jet_scheduler_native_publish(record); }
        #[cfg(jet_native_pack)]
        {
            unsafe extern "C" {
                fn jet_rt_control_publish_stop(kind: i64, jct: i64, owner: i64, start: i64, end: i64, bytes: *const u8, len: usize, form: i64) -> bool;
            }
            let kind = match record.kind {
                super::JetStopKind::Cancelled => 1,
                super::JetStopKind::Deadline => 2,
                super::JetStopKind::BudgetFuel => 3,
                super::JetStopKind::BudgetDepth => 4,
                super::JetStopKind::Panic => 5,
                super::JetStopKind::HostFault | super::JetStopKind::Trap => unreachable!("fatal comptime record on recoverable budget rail"),
            };
            unsafe {
                jet_rt_control_publish_stop(kind, record.jct_status.map_or(-1, i64::from),
                    record.owner.map_or(-1, |word| word as i64), record.start.map_or(-1, |word| word as i64),
                    record.end.map_or(-1, |word| word as i64), record.message.as_ptr(), record.message.len(), 0);
            }
        }
    }

    // Fatal hook (jet_native_comptime_fail): status 1 runtime panic, 4
    // foreign/host panic. Writes the stop record and exits.
    fn comptime_fatal(status: u32, message: &str) -> ! {
        let kind = if status == 4 { super::JetStopKind::HostFault } else { super::JetStopKind::Panic };
        super::jet_native_comptime_channel_write(&comptime_site(kind, status, message.to_string()).encode());
        super::jet_runtime_finalized_exit(if status == 4 { 101 } else { 80 + status as i32 })
    }

    // The request's budget stop: the first one wins (from any thread, at that
    // thread's frame), the remaining fuel is spent so every thread's next
    // charge stops too, and the first record goes on the current control.
    fn comptime_budget_stop(kind: super::JetStopKind, status: u32) {
        let record = COMPTIME_STOP.lock().unwrap_or_else(|poison| poison.into_inner())
            .get_or_insert_with(|| comptime_site(kind, status, String::new()))
            .clone();
        super::jet_native_comptime_spend_all();
        COMPTIME_STOPPED.store(true, std::sync::atomic::Ordering::Release);
        comptime_publish_stop(record);
    }

    fn comptime_stop() -> Option<super::JetStopRecord> {
        COMPTIME_STOP.lock().unwrap_or_else(|poison| poison.into_inner()).clone()
    }

    /// Charges `units` of comptime fuel from a runtime kernel; true when the
    /// request's budget stop is now published on the current control.
    pub(crate) fn comptime_charge(units: u64) -> bool {
        if super::jet_native_comptime_work(units) {
            comptime_budget_stop(super::JetStopKind::BudgetFuel, 2);
            return true;
        }
        false
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_comptime_begin(fuel: u64, max_depth: u64) -> bool {
        guard(|| {
            if !super::jet_native_comptime_begin(fuel, max_depth, comptime_fatal) { return false; }
            EXECUTION_COMPTIME.store(true, std::sync::atomic::Ordering::Release);
            *COMPTIME_STOP.lock().unwrap_or_else(|poison| poison.into_inner()) = None;
            COMPTIME_STOPPED.store(false, std::sync::atomic::Ordering::Release);
            let scope = super::JetNativeTaskControlScope::enter(super::JetTaskControl::new());
            COMPTIME_SCOPE.with(|slot| *slot.borrow_mut() = Some(scope));
            true
        })
    }
    #[no_mangle]
    pub extern "C" fn jet_rt_comptime_step(owner: u64, start: u64, end: u64) {
        if super::jet_native_comptime_step(owner, start, end) {
            comptime_budget_stop(super::JetStopKind::BudgetFuel, 2);
        }
    }
    #[no_mangle]
    pub extern "C" fn jet_rt_comptime_work(units: u64) { let _ = comptime_charge(units); }
    #[no_mangle]
    pub extern "C" fn jet_rt_comptime_enter(owner: u64, start: u64, end: u64) {
        if super::jet_native_comptime_enter(owner, start, end) {
            comptime_budget_stop(super::JetStopKind::BudgetDepth, 3);
        } else if COMPTIME_STOPPED.load(std::sync::atomic::Ordering::Acquire) {
            // Another thread spent the request's budget.
            if let Some(record) = comptime_stop() { comptime_publish_stop(record); }
        }
    }
    #[no_mangle]
    pub extern "C" fn jet_rt_comptime_leave() { super::jet_native_comptime_leave(); }
    #[no_mangle]
    pub extern "C" fn jet_rt_comptime_end() {
        super::jet_native_comptime_end();
        let scope = COMPTIME_SCOPE.with(|slot| slot.borrow_mut().take());
        drop(scope);
    }
    /// Writes the request's budget stop record to the private channel and
    /// returns its JCT1 status (2 fuel, 3 depth); 0 when the request did not
    /// stop on its budget.
    #[no_mangle]
    pub extern "C" fn jet_rt_comptime_stop_write() -> u32 {
        let Some(record) = comptime_stop() else { return 0 };
        super::jet_native_comptime_channel_write(&record.encode());
        record.jct_status.unwrap_or(0)
    }

    // Raw request-scope leaves for the Jet execution service. Both modes own
    // exactly one canonical task control. A turn has no comptime site: a fatal
    // stop MUST NOT fabricate owner/span zeroes. Unlimited turn counters only
    // install the canonical fatal hook; ordinary native turns have no budget
    // operations in their MIR.
    #[no_mangle]
    pub extern "C" fn jet_rt_execution_begin(fuel: u64, depth: u64, comptime: u64) -> bool {
        let started = jet_rt_comptime_begin(fuel, depth);
        if started { EXECUTION_COMPTIME.store(comptime != 0, std::sync::atomic::Ordering::Release); }
        started
    }
    #[no_mangle]
    pub extern "C" fn jet_rt_execution_end() {
        use std::io::Write;
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();
        jet_rt_comptime_end();
    }
    #[no_mangle]
    pub extern "C" fn jet_rt_execution_stop_write() -> u64 {
        let Some(record) = super::jet_scheduler_native_stop_record() else { return 0 };
        super::jet_native_comptime_channel_write(&record.encode());
        1
    }

    /// Plain internal runtime imports for MIR Poll and Stopped. Neither
    /// consumes the status nor alters Task.cancel's wait-point delivery.
    #[cfg(not(jet_native_pack))]
    #[no_mangle]
    pub extern "C" fn jet_rt_session_poll() -> u64 {
        super::jet_scheduler_native_poll()
    }
    #[cfg(not(jet_native_pack))]
    #[no_mangle]
    pub extern "C" fn jet_rt_session_propagate() -> u64 {
        super::jet_scheduler_native_propagate()
    }
    #[cfg(not(jet_native_pack))]
    #[no_mangle]
    pub extern "C" fn jet_rt_session_status() -> u64 {
        super::jet_scheduler_native_status()
    }

    // These are the canonical nesting Shield operations, not a second mask.
    #[cfg(not(jet_native_pack))]
    #[no_mangle]
    pub extern "C" fn jet_rt_session_shield_enter() {
        guard(super::jet_scheduler_shield_enter)
    }
    #[cfg(not(jet_native_pack))]
    #[no_mangle]
    pub extern "C" fn jet_rt_session_shield_leave() {
        guard(super::jet_scheduler_shield_leave)
    }

    // Entry error edge (MIRRust.rs entry_error_exit): a failing entry ends
    // the process with its report and exit status 1.

    /// Ends the process with the report of `error` (taken, from
    /// jet_rt_record_new_jet_err).
    #[cfg(not(jet_native_pack))]
    #[no_mangle]
    pub extern "C" fn jet_rt_entry_error_exit_err(error: *mut super::JetErr) {
        let error = take_record(error);
        guard(move || -> () { super::jet_entry_error_exit_jet(error) })
    }

    /// Ends the process with the entry report of an error's Printable text.
    #[cfg(not(jet_native_pack))]
    #[no_mangle]
    pub extern "C" fn jet_rt_entry_error_exit(text: JetCString) {
        let text = view(text).to_owned();
        guard(move || -> () { super::jet_entry_error_exit(text) })
    }

    // Core records (Compiler/JetBackend/Source/Lower/Records.jet): a
    // MIR-defined Core type crosses a route as a handle to the runtime's own
    // value. `jet_rt_record_tag_<R>` returns a payload enum's variant index;
    // a getter borrows the handle and returns an owned carrier (an Option
    // getter returns its tag and writes the payload through `some`); a
    // constructor borrows String handles and Option boxes of a String or Int
    // (null is None; the payload is the box's first word) and takes nested
    // record handles. A fieldless Core enum member crosses as its index.

    fn record<T>(value: T) -> *mut T {
        Box::into_raw(Box::new(value))
    }

    fn take_record<T>(value: *mut T) -> T {
        // SAFETY: the caller transfers its owned handle from `record`.
        *unsafe { Box::from_raw(value) }
    }

    fn option_text(boxed: *const JetCString) -> super::JetOutcome<String, super::JetAbsent> {
        // SAFETY: a non-null Option box holds a live String handle first.
        if boxed.is_null() { Err(super::JetAbsent) } else { Ok(view(unsafe { *boxed }).to_owned()) }
    }


    fn some_text(some: *mut JetCString, value: &super::JetOutcome<String, super::JetAbsent>) -> i64 {
        match value {
            // SAFETY: `some` is the caller's zeroed out slot.
            Ok(text) => { unsafe { some.write(handle(text.clone())) }; 1 }
            Err(_) => 0,
        }
    }

    fn record_index_stop() -> ! {
        super::jet_panic("<core.prelude>", 0, "record variant index out of range")
    }

    // Same-name canonical Err88 provider owns this ABI in native packs.
    #[cfg(not(jet_native_pack))]
    #[no_mangle]
    pub extern "C" fn jet_rt_record_new_jet_err(message: JetCString, code: *const JetCString, cause: *mut super::JetErr) -> *mut super::JetErr {
        guard(|| {
            let cause = if cause.is_null() { Err(super::JetAbsent) } else { Ok(take_record(cause)) };
            record(super::jet_err(view(message).to_owned(), option_text(code), cause))
        })
    }

    #[cfg(not(jet_native_pack))]
    #[no_mangle]
    pub extern "C" fn jet_rt_record_get_jet_err_message(error: *mut super::JetErr) -> JetCString {
        // SAFETY: `error` is a live handle the caller lends for this call.
        guard(|| handle(super::jet_err_message(unsafe { &*error })))
    }

    #[cfg(not(jet_native_pack))]
    #[no_mangle]
    pub extern "C" fn jet_rt_record_get_jet_err_code(error: *mut super::JetErr, some: *mut JetCString) -> i64 {
        // SAFETY: `error` is a live handle the caller lends for this call.
        guard(|| some_text(some, &super::jet_err_code(unsafe { &*error })))
    }

    #[cfg(not(jet_native_pack))]
    #[no_mangle]
    pub extern "C" fn jet_rt_record_get_jet_err_cause(error: *mut super::JetErr, some: *mut *mut super::JetErr) -> i64 {
        // SAFETY: `error` is a live handle the caller lends; `some` is its zeroed out slot.
        guard(|| match super::jet_err_cause(unsafe { &*error }) {
            Ok(cause) => { unsafe { some.write(record(cause)) }; 1 }
            Err(_) => 0,
        })
    }

    // Seed/verification bodies die at cutover. Native packs use the Jet
    // definitions; both tiers share C headers, never Rust Box/enum layout.
    #[cfg(not(jet_native_pack))]
    mod host_records {
        use super::*;
        use crate::jet_c_abi_host::{JetCIOContext, JetCIOError, JetCTaskFailure};

        fn carrier<T>(value: T) -> *mut T {
            // SAFETY: one fresh allocation exactly matching the C header.
            let pointer = unsafe { jet_rt_alloc(std::mem::size_of::<T>(), std::mem::align_of::<T>()) }.cast::<T>();
            unsafe { pointer.write(value) };
            pointer
        }

        fn clone_text(text: JetCString) -> JetCString {
            if text.is_null() { std::ptr::null_mut() } else { text_handle(view(text)) }
        }

        fn option_clone(boxed: *const JetCString) -> JetCString {
            // SAFETY: a present option lends its initialized text word.
            if boxed.is_null() { std::ptr::null_mut() } else { clone_text(unsafe { *boxed }) }
        }

        fn text_out(text: JetCString, some: *mut JetCString) -> i64 {
            if text.is_null() { return 0; }
            unsafe { some.write(clone_text(text)) };
            1
        }

        #[no_mangle]
        pub extern "C" fn jet_rt_record_new_io_context(operation: i64, resource: *const JetCString, os_code: *const i64, cause: *const JetCString) -> *mut JetCIOContext {
            guard(|| {
                if !(0..8).contains(&operation) { record_index_stop(); }
                carrier(JetCIOContext {
                    operation, resource: option_clone(resource),
                    has_os_code: i64::from(!os_code.is_null()),
                    os_code: if os_code.is_null() { 0 } else { native(unsafe { *os_code }) },
                    cause: option_clone(cause),
                })
            })
        }

        #[no_mangle]
        pub extern "C" fn jet_rt_record_get_io_context_operation(context: *mut JetCIOContext) -> i64 {
            guard(|| unsafe { (*context).operation })
        }

        #[no_mangle]
        pub extern "C" fn jet_rt_record_get_io_context_resource(context: *mut JetCIOContext, some: *mut JetCString) -> i64 {
            guard(|| text_out(unsafe { (*context).resource }, some))
        }

        #[no_mangle]
        pub extern "C" fn jet_rt_record_get_io_context_os_code(context: *mut JetCIOContext, some: *mut i64) -> i64 {
            guard(|| {
                let value = unsafe { &*context };
                if value.has_os_code == 0 { return 0; }
                unsafe { some.write(crate::jet_std::jet_int_from_i64(value.os_code)) };
                1
            })
        }

        #[no_mangle]
        pub extern "C" fn jet_rt_record_get_io_context_cause(context: *mut JetCIOContext, some: *mut JetCString) -> i64 {
            guard(|| text_out(unsafe { (*context).cause }, some))
        }

        #[no_mangle]
        pub extern "C" fn jet_rt_record_tag_io_error(error: *mut JetCIOError) -> i64 {
            guard(|| unsafe { (*error).kind })
        }

        // Projection deep-clones the five payload words; construction
        // transfers children and frees only the consumed context header.
        macro_rules! io_error_context_variants {
            ($($tag:literal: $get:ident, $new:ident;)*) => {$(
                #[no_mangle]
                pub extern "C" fn $get(error: *mut JetCIOError) -> *mut JetCIOContext {
                    guard(|| {
                        let value = unsafe { &*error };
                        if value.kind != $tag { record_index_stop(); }
                        carrier(JetCIOContext {
                            operation: value.operation, resource: clone_text(value.resource),
                            has_os_code: value.has_os_code, os_code: value.os_code,
                            cause: clone_text(value.cause),
                        })
                    })
                }

                #[no_mangle]
                pub extern "C" fn $new(context: *mut JetCIOContext) -> *mut JetCIOError {
                    guard(|| {
                        // SAFETY: consume the header, transferring its children.
                        let value = unsafe { context.read() };
                        unsafe { jet_rt_free(context.cast(), 40, 8) };
                        carrier(JetCIOError {
                            kind: $tag, operation: value.operation, resource: value.resource,
                            has_os_code: value.has_os_code, os_code: value.os_code,
                            cause: value.cause, limit: 0,
                        })
                    })
                }
            )*};
        }

        io_error_context_variants! {
            0: jet_rt_record_get_io_error_invalid_input, jet_rt_record_new_io_error_invalid_input;
            1: jet_rt_record_get_io_error_not_found, jet_rt_record_new_io_error_not_found;
            2: jet_rt_record_get_io_error_permission_denied, jet_rt_record_new_io_error_permission_denied;
            3: jet_rt_record_get_io_error_timed_out, jet_rt_record_new_io_error_timed_out;
            4: jet_rt_record_get_io_error_cancelled, jet_rt_record_new_io_error_cancelled;
            5: jet_rt_record_get_io_error_closed, jet_rt_record_new_io_error_closed;
            6: jet_rt_record_get_io_error_protocol, jet_rt_record_new_io_error_protocol;
            7: jet_rt_record_get_io_error_other, jet_rt_record_new_io_error_other;
            9: jet_rt_record_get_io_error_already_exists, jet_rt_record_new_io_error_already_exists;
            10: jet_rt_record_get_io_error_unsupported, jet_rt_record_new_io_error_unsupported;
        }

        #[no_mangle]
        pub extern "C" fn jet_rt_record_get_io_error_resource_limit(error: *mut JetCIOError) -> i64 {
            guard(|| {
                let value = unsafe { &*error };
                if value.kind != 8 { record_index_stop(); }
                value.limit
            })
        }

        #[no_mangle]
        pub extern "C" fn jet_rt_record_new_io_error_resource_limit(limit: i64) -> *mut JetCIOError {
            guard(|| {
                if !(0..5).contains(&limit) { record_index_stop(); }
                carrier(JetCIOError {
                    kind: 8, operation: 0, resource: std::ptr::null_mut(),
                    has_os_code: 0, os_code: 0, cause: std::ptr::null_mut(), limit,
                })
            })
        }

        #[no_mangle]
        pub extern "C" fn jet_rt_record_tag_jet_task_failure(failure: *mut JetCTaskFailure) -> i64 {
            guard(|| unsafe { (*failure).tag })
        }

        #[no_mangle]
        pub extern "C" fn jet_rt_record_get_jet_task_failure_panicked(failure: *mut JetCTaskFailure) -> JetCString {
            guard(|| {
                let value = unsafe { &*failure };
                if value.tag != 2 { record_index_stop(); }
                clone_text(value.reason)
            })
        }

        #[no_mangle]
        pub extern "C" fn jet_rt_record_new_jet_task_failure_cancelled() -> *mut JetCTaskFailure {
            guard(|| carrier(JetCTaskFailure { tag: 0, reason: std::ptr::null_mut() }))
        }

        #[no_mangle]
        pub extern "C" fn jet_rt_record_new_jet_task_failure_deadline_blown() -> *mut JetCTaskFailure {
            guard(|| carrier(JetCTaskFailure { tag: 1, reason: std::ptr::null_mut() }))
        }

        #[no_mangle]
        pub extern "C" fn jet_rt_record_new_jet_task_failure_panicked(reason: JetCString) -> *mut JetCTaskFailure {
            guard(|| carrier(JetCTaskFailure { tag: 2, reason: clone_text(reason) }))
        }
    }


    // One decimal digit needs at most one bit, so this stack bound covers the
    // complete usize range on every target without a heap-backed formatter.
    #[cfg(not(jet_native_pack))]
    const ALLOCATION_FAILURE_CAPACITY: usize =
        b"memory allocation of ".len() + usize::BITS as usize + b" bytes failed\n".len();

    #[cfg(not(jet_native_pack))]
    fn allocation_failure_message(mut size: usize, buffer: &mut [u8; ALLOCATION_FAILURE_CAPACITY]) -> &[u8] {
        let suffix = b" bytes failed\n";
        let mut start = buffer.len() - suffix.len();
        buffer[start..].copy_from_slice(suffix);
        loop {
            start -= 1;
            buffer[start] = b'0' + (size % 10) as u8;
            size /= 10;
            if size == 0 { break; }
        }
        let prefix = b"memory allocation of ";
        start -= prefix.len();
        buffer[start..start + prefix.len()].copy_from_slice(prefix);
        &buffer[start..]
    }

    #[cfg(not(jet_native_pack))]
    #[cold]
    fn allocation_failure(size: usize) -> ! {
        // OOM is process-fatal, not a Jet panic or a task stop. In particular,
        // Rust's allocation-error/panic hook must not add a backtrace note.
        let mut buffer = [0u8; ALLOCATION_FAILURE_CAPACITY];
        let message = allocation_failure_message(size, &mut buffer);
        let _ = super::jet_term_write_stderr_bytes(message, true);
        std::process::abort()
    }

    // Rust-tier seed/verification path only; dies when that tier is cut over.
    // Native packs have one allocator definition, in the Jet runtime object.
    #[cfg(not(jet_native_pack))]
    #[no_mangle]
    pub extern "C" fn jet_rt_alloc(size: usize, align: usize) -> *mut u8 {
        guard(|| {
            let layout = std::alloc::Layout::from_size_align(size.max(1), align)
                .unwrap_or_else(|_| super::jet_panic("<core.prelude>", 0, "invalid box layout"));
            // SAFETY: the layout has a non-zero size.
            let ptr = unsafe { std::alloc::alloc(layout) };
            if ptr.is_null() {
                allocation_failure(layout.size());
            }
            ptr
        })
    }

    #[cfg(not(jet_native_pack))]
    #[no_mangle]
    pub extern "C" fn jet_rt_free(ptr: *mut u8, size: usize, align: usize) {
        guard(|| {
            if ptr.is_null() {
                return;
            }
            let layout = std::alloc::Layout::from_size_align(size.max(1), align)
                .unwrap_or_else(|_| super::jet_panic("<core.prelude>", 0, "invalid box layout"));
            // SAFETY: `ptr` came from `jet_rt_alloc` with this size and align.
            unsafe { std::alloc::dealloc(ptr, layout) }
        })
    }

    // Native C carriers import the Jet heap and retain its init-array entry.
    #[cfg(jet_native_pack)]
    unsafe extern "C" {
        pub(super) fn jet_rt_alloc(size: usize, align: usize) -> *mut u8;
        fn jet_rt_free(ptr: *mut u8, size: usize, align: usize);
    }


    // Display text, as generated Rust renders `print` and interpolation.

    #[cfg(not(jet_native_pack))]
    #[no_mangle]
    pub extern "C" fn jet_rt_int_to_string(value: i64) -> JetCString {
        guard(|| handle(crate::jet_std::jet_int_to_string(value)))
    }

    #[cfg(not(jet_native_pack))]
    #[no_mangle]
    pub extern "C" fn jet_rt_i64_to_string(value: i64) -> JetCString {
        guard(|| handle(value.to_string()))
    }

    #[cfg(not(jet_native_pack))]
    #[no_mangle]
    pub extern "C" fn jet_rt_u64_to_string(value: u64) -> JetCString {
        guard(|| handle(value.to_string()))
    }

    #[no_mangle]
    pub extern "C" fn jet_rt_char_to_string(value: i64) -> JetCString {
        guard(|| {
            let scalar = u32::try_from(value).ok().and_then(char::from_u32);
            handle(scalar.unwrap_or(char::REPLACEMENT_CHARACTER).to_string())
        })
    }

    #[cfg(not(jet_native_pack))]
    #[no_mangle]
    pub extern "C" fn jet_rt_bool_to_string(value: u8) -> JetCString {
        guard(|| handle((value != 0).to_string()))
    }

    #[cfg(not(jet_native_pack))]
    /// A Float arrives in xmm0 as the double itself.
    #[no_mangle]
    pub extern "C" fn jet_rt_float_to_string(value: f64) -> JetCString {
        guard(|| handle(super::jet_fmt_display(&value)))
    }

    #[cfg(not(jet_native_pack))]
    /// A Float's Debug text (`JetDebug for f64`, which differs from Display).
    #[no_mangle]
    pub extern "C" fn jet_rt_float_debug(value: f64) -> JetCString {
        guard(|| handle(super::JetDebug::jet_debug(&value)))
    }


    /// A Prelude `jet_panic("<core.prelude>", 0, message)` stop that generated
    /// code checks inline (e.g. an iterator loop stride, Collections.rs:1518).
    #[no_mangle]
    pub extern "C" fn jet_rt_panic_message(ptr: *const u8, len: usize) {
        let message = text(ptr, len).into_owned();
        guard(move || -> () { super::jet_panic("<core.prelude>", 0, &message) })
    }

    // Prelude routes: the C name is the last `::` segment of the MIR symbol.

    macro_rules! exact_int_binary {
        ($($name:ident),* $(,)?) => {$(
            #[no_mangle]
            pub extern "C" fn $name(left: i64, right: i64) -> i64 {
                guard(|| crate::jet_std::$name(left, right))
            }
        )*};
    }

    macro_rules! exact_int_located {
        ($($name:ident),* $(,)?) => {$(
            #[no_mangle]
            pub extern "C" fn $name(left: i64, right: i64, file_ptr: *const u8, file_len: usize, line: i64) -> i64 {
                guard(|| crate::jet_std::$name(left, right, &text(file_ptr, file_len), line_of(line)))
            }
        )*};
    }

    #[cfg(not(jet_native_pack))]
    exact_int_binary!(jet_int_add, jet_int_sub, jet_int_mul, jet_int_bit_and, jet_int_bit_or, jet_int_bit_xor, jet_int_compare);
    #[cfg(not(jet_native_pack))]
    exact_int_located!(jet_int_div, jet_int_rem, jet_int_floor_div, jet_int_mod, jet_int_pow, jet_int_shl, jet_int_shr, jet_int_div_euclid, jet_int_rem_euclid);

    // Fixed routes that cannot use the backend's inline arithmetic path.
    // The ABI transports normalized words and a borrowed source-location
    // slice; arithmetic remains in the canonical concrete Prelude kernel.
    macro_rules! fixed_int_located {
        ($ty:ty; $($name:ident),* $(,)?) => {$(
            #[no_mangle]
            pub extern "C" fn $name(left: i64, right: i64, file_ptr: *const u8, file_len: usize, line: i64) -> i64 {
                guard(|| super::$name(left as $ty, right as $ty, &text(file_ptr, file_len), line_of(line)) as i64)
            }
        )*};
    }
    fixed_int_located!(i8; jet_i8_trap_div, jet_i8_trap_rem, jet_i8_trap_floor_div, jet_i8_trap_mod, jet_i8_trap_pow);
    fixed_int_located!(i16; jet_i16_trap_div, jet_i16_trap_rem, jet_i16_trap_floor_div, jet_i16_trap_mod, jet_i16_trap_pow);
    fixed_int_located!(i32; jet_i32_trap_div, jet_i32_trap_rem, jet_i32_trap_floor_div, jet_i32_trap_mod, jet_i32_trap_pow);
    fixed_int_located!(i64; jet_i64_trap_div, jet_i64_trap_rem, jet_i64_trap_floor_div, jet_i64_trap_mod, jet_i64_trap_pow);
    fixed_int_located!(u8; jet_u8_trap_div, jet_u8_trap_rem, jet_u8_trap_floor_div, jet_u8_trap_mod, jet_u8_trap_pow);
    fixed_int_located!(u16; jet_u16_trap_div, jet_u16_trap_rem, jet_u16_trap_floor_div, jet_u16_trap_mod, jet_u16_trap_pow);
    fixed_int_located!(u32; jet_u32_trap_div, jet_u32_trap_rem, jet_u32_trap_floor_div, jet_u32_trap_mod, jet_u32_trap_pow);
    fixed_int_located!(u64; jet_u64_trap_div, jet_u64_trap_rem, jet_u64_trap_floor_div, jet_u64_trap_mod, jet_u64_trap_pow);

    #[no_mangle]
    pub extern "C" fn jet_numeric_bit_count(value: i64, operation: i64, width: i64) -> i64 {
        guard(|| crate::jet_foundation::NumericConversion::jet_numeric_bit_count(value, native(operation), native(width)))
    }

    /// An exact Int converted to a fixed-width integer (`int_checked_fixed`,
    /// stopping at the source location when out of range). Every fixed width
    /// the native backend lowers is at most 64 bits, so the checked value is
    /// its word (two's complement for a U64 above I64.MAX).
    #[cfg(not(jet_native_pack))]
    #[no_mangle]
    pub extern "C" fn jet_int_checked_fixed(value: i64, kind: i64, file_ptr: *const u8, file_len: usize, line: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_checked_fixed(value, kind, &text(file_ptr, file_len), line_of(line)) as i64)
    }

    #[cfg(not(jet_native_pack))]
    #[no_mangle]
    pub extern "C" fn jet_int_owned_from_i64(value: i64) -> i64 {
        guard(|| crate::jet_std::jet_int_owned_from_i64(value).into_raw())
    }

    /// A copy of an exact Int carrier word (a big-integer owner gains a reference).
    #[cfg(not(jet_native_pack))]
    #[no_mangle]
    pub extern "C" fn jet_rt_int_clone(value: i64) -> i64 {
        // SAFETY: `value` is a live exact-Int carrier word the caller owns.
        guard(|| unsafe { crate::jet_foundation::Numeric::JetInt::clone_from_raw(value) }.into_raw())
    }

    /// Rust-tier seed owner release. Native packs use Int.jet's atomic drop.
    #[cfg(not(jet_native_pack))]
    #[no_mangle]
    pub extern "C" fn jet_rt_int_drop(value: i64) {
        guard(|| unsafe { drop(crate::jet_foundation::Numeric::JetInt::from_raw_owned(value)) })
    }

    /// Remaining memory leaf: ONE domain while Rust atomic consumers remain.
    /// Native Int.jet calls only after the final CAS has changed refs to zero.
    #[cfg(jet_native_pack)]
    #[no_mangle]
    pub extern "C" fn jet_rt_int_retire_zero(node: i64) {
        guard(|| unsafe { crate::jet_foundation::Numeric::jet_int_retire_raw_zero(node) })
    }

    /// An exact Int constant beyond the inline word range, from its decimal
    /// text (MIR `BigInt` constants; the JetBackend builds them at use).
    #[cfg(not(jet_native_pack))]
    #[no_mangle]
    pub extern "C" fn jet_rt_int_from_static(ptr: *const u8, len: usize) -> i64 {
        guard(|| crate::jet_std::jet_int_from_str(&text(ptr, len)).unwrap_or_else(|message| panic!("{message}")))
    }

    #[cfg(not(jet_native_pack))]
    #[no_mangle]
    pub extern "C" fn jet_int_owned_to_i64(value: i64) -> i64 {
        guard(|| native(value))
    }

    #[cfg(not(jet_native_pack))]
    #[no_mangle]
    pub extern "C" fn jet_int_owned_to_f64(value: i64) -> f64 {
        guard(|| crate::jet_std::jet_int_to_f64(value))
    }

    #[no_mangle]
    pub extern "C" fn jet_term_write_stdout_line(value: JetCString, flush: u8) {
        // Generated Rust ignores the stream result of `print` the same way.
        guard(|| {
            let _ = super::jet_term_write_stdout_line(view(value), flush != 0);
        })
    }

    #[cfg(not(jet_native_pack))]
    #[no_mangle]
    pub extern "C" fn jet_loop_range_init(
        start: i64,
        end: i64,
        step: i64,
        has_step: u8,
        exclusive: u8,
    ) -> *mut super::JetLoopRangeCursor {
        guard(|| {
            let step = if has_step != 0 { native(step) } else { 1 };
            Box::into_raw(Box::new(super::jet_loop_range_init(
                native(start),
                native(end),
                step,
                has_step != 0,
                exclusive != 0,
            )))
        })
    }

    #[cfg(not(jet_native_pack))]
    fn cursor<'a>(value: *mut super::JetLoopRangeCursor) -> &'a mut super::JetLoopRangeCursor {
        // SAFETY: `value` is a live cursor from `jet_loop_range_init` that the
        // caller owns for the duration of this call.
        unsafe { &mut *value }
    }

    #[cfg(not(jet_native_pack))]
    #[no_mangle]
    pub extern "C" fn jet_loop_range_has_next(value: *mut super::JetLoopRangeCursor) -> bool {
        guard(|| super::jet_loop_range_has_next(cursor(value)))
    }

    #[cfg(not(jet_native_pack))]
    #[no_mangle]
    pub extern "C" fn jet_loop_range_value(value: *mut super::JetLoopRangeCursor) -> i64 {
        guard(|| crate::jet_std::jet_int_from_i64(super::jet_loop_range_value(cursor(value))))
    }

    #[cfg(not(jet_native_pack))]
    #[no_mangle]
    pub extern "C" fn jet_loop_range_advance(value: *mut super::JetLoopRangeCursor) {
        guard(|| super::jet_loop_range_advance(cursor(value)))
    }

    #[cfg(not(jet_native_pack))]
    #[no_mangle]
    pub extern "C" fn jet_loop_cursor_drop(value: *mut super::JetLoopRangeCursor) {
        if !value.is_null() {
            // SAFETY: the caller transfers its owned cursor.
            drop(unsafe { Box::from_raw(value) });
        }
    }

    // #29 host seam: L7b replaces this raw OS-presence query at Rust cutover.
    // Unlike env_get, presence is independent of the value's UTF-8 validity.
    #[no_mangle]
    pub unsafe extern "C" fn jet_rt_env_present(key: JetCString) -> bool {
        guard(|| std::env::var_os(view(key)).is_some())
    }
}
