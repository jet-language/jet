/// The one signal-handler mechanism (#2027, I8 + I9).
///
/// This file is the single home for every signal fact: the pending interrupt
/// count, typed process-signal bits, the platform handler, the arm path, and
/// the consumption rules. Nothing outside this file may declare a second
/// signal mark function or a second install path.
///
/// Three tiers reach it and none of them restates it:
///   * AOT — `Prelude/CoreLib/Top/FSIoEnvOsTesting.rs`'s `jet_os_interrupt`,
///     which the generated program embeds with the process/filesystem adapter
///     closure (`needs_process || needs_fs_runtime`);
///     the full dispatcher remains the one shared implementation.
///   * the resident Cranelift host — `jet-jit/src/CoreHost.rs`, via
///     `jet_codegen::interrupt_runtime`;
///   * the TIR evaluator ambient — `Codegen/TIR/eval/mod.rs`, via
///     `crate::interrupt_runtime`.
///
/// The last two share ONE compiled instance (`jet-codegen/src/lib.rs`,
/// `pub mod interrupt_runtime`), so the `jet` binary has one pending state,
/// one signal disposition, and one arm result no matter which tier is running.
/// `signal(SIGINT, …)` replaces the process handler, so a second instance
/// would silently disarm the first tier's queue.
///
/// A tier supplies only its Ctrl-C handler storage and invocation adapter.
/// Typed process signals use a shared cancellation callback instead: it looks
/// up the current root task control when delivered, so a resident run cannot
/// keep a prior run alive.
///
/// `#Shield` never enters this file, by construction. A shield defers a
/// *cooperative* interrupt — a cancel or a blown deadline — at the wait points
/// of the shielded task (`Prelude/Scheduler.rs::jet_scheduler_shielded`). A
/// process signal marks cancellation through the same task-control carrier;
/// the cancellation is delivered by the scheduler at its next unshielded wait
/// point.
pub fn jet_interrupt_poll_interval() -> std::time::Duration {
    std::time::Duration::from_millis(10)
}

pub fn jet_interrupt_core_error(message: &str) -> String {
    format!("core.sys.on_interrupt: {message}")
}

pub fn jet_process_signal_error(message: &str) -> String {
    format!("core.process.on_signal: {message}")
}

pub fn jet_interrupt_dispatcher_start_error(error: impl std::fmt::Display) -> String {
    format!("could not start interrupt dispatcher: {error}")
}

pub fn jet_interrupt_dispatcher_stopped_error() -> &'static str {
    "interrupt dispatcher stopped"
}

pub fn jet_interrupt_invalid_callback_record_error() -> &'static str {
    "invalid interrupt callback record"
}

pub fn jet_interrupt_invalid_callback_value_error() -> &'static str {
    "core.sys.on_interrupt callback"
}

pub fn jet_interrupt_unavailable_error() -> &'static str {
    "interrupt handling is unavailable on this target"
}

// Keep the process-wide state in one carrier. The first field preserves the
// count-first behavior of `core.sys.on_interrupt`; the second coalesces typed
// process signals because cancellation is idempotent.
const JET_SIGNAL_INT: usize = 1 << 0;
const JET_SIGNAL_HUP: usize = 1 << 1;
const JET_SIGNAL_TERM: usize = 1 << 2;
const JET_SIGNAL_ALL: usize = JET_SIGNAL_INT | JET_SIGNAL_HUP | JET_SIGNAL_TERM;

#[doc(hidden)]
pub type JetProcessSignalCallback = std::sync::Arc<dyn Fn() + Send + Sync + 'static>;

struct JetInterruptState {
    pending_interrupts: std::sync::atomic::AtomicUsize,
    pending_process_signals: std::sync::atomic::AtomicUsize,
    installed: std::sync::atomic::AtomicUsize,
    install_lock: std::sync::Mutex<()>,
    process_dispatch_lock: std::sync::Mutex<()>,
    process_handlers: std::sync::Mutex<[Option<JetProcessSignalCallback>; 3]>,
}

static JET_INTERRUPT_STATE: std::sync::LazyLock<JetInterruptState> =
    std::sync::LazyLock::new(|| JetInterruptState {
        pending_interrupts: std::sync::atomic::AtomicUsize::new(0),
        pending_process_signals: std::sync::atomic::AtomicUsize::new(0),
        installed: std::sync::atomic::AtomicUsize::new(0),
        install_lock: std::sync::Mutex::new(()),
        process_dispatch_lock: std::sync::Mutex::new(()),
        process_handlers: std::sync::Mutex::new([None, None, None]),
    });

fn jet_process_signal_index(mask: usize) -> Option<usize> {
    match mask {
        JET_SIGNAL_INT => Some(0),
        JET_SIGNAL_HUP => Some(1),
        JET_SIGNAL_TERM => Some(2),
        _ => None,
    }
}

#[cfg(unix)]
fn jet_interrupt_signal_bit(signal: i32) -> usize {
    match signal {
        2 => JET_SIGNAL_INT,
        1 => JET_SIGNAL_HUP,
        15 => JET_SIGNAL_TERM,
        _ => 0,
    }
}

/// The platform callback. It does no allocation, no locking, and no user work:
/// one relaxed atomic update per applicable signal is the whole async-signal-
/// safe body.
#[cfg(unix)]
extern "C" fn jet_interrupt_mark(signal: i32) {
    let state = &*JET_INTERRUPT_STATE;
    if signal == 2 {
        state
            .pending_interrupts
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    let bit = jet_interrupt_signal_bit(signal);
    if bit != 0 {
        state
            .pending_process_signals
            .fetch_or(bit, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(windows)]
unsafe extern "system" fn jet_interrupt_mark(kind: u32) -> i32 {
    const CTRL_C_EVENT: u32 = 0;
    if kind == CTRL_C_EVENT {
        let state = &*JET_INTERRUPT_STATE;
        state
            .pending_interrupts
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        state
            .pending_process_signals
            .fetch_or(JET_SIGNAL_INT, std::sync::atomic::Ordering::Relaxed);
        1
    } else {
        0
    }
}

#[cfg(unix)]
fn jet_interrupt_arm_mask(mask: usize) -> Result<(), String> {
    if mask & !JET_SIGNAL_ALL != 0 {
        return Err("invalid process signal".to_string());
    }
    extern "C" {
        fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
    }
    let state = &*JET_INTERRUPT_STATE;
    let _lock = state
        .install_lock
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let mut installed = state
        .installed
        .load(std::sync::atomic::Ordering::Acquire);
    for (bit, number, name) in [
        (JET_SIGNAL_INT, 2, "SIGINT"),
        (JET_SIGNAL_HUP, 1, "SIGHUP"),
        (JET_SIGNAL_TERM, 15, "SIGTERM"),
    ] {
        if mask & bit == 0 || installed & bit != 0 {
            continue;
        }
        let previous = unsafe { signal(number, jet_interrupt_mark) };
        if previous == usize::MAX {
            return Err(format!("could not install the {name} handler"));
        }
        installed |= bit;
        state
            .installed
            .store(installed, std::sync::atomic::Ordering::Release);
    }
    Ok(())
}

#[cfg(windows)]
fn jet_interrupt_arm_mask(mask: usize) -> Result<(), String> {
    if mask & !(JET_SIGNAL_INT) != 0 {
        return Err(jet_interrupt_unavailable_error().to_string());
    }
    extern "system" {
        fn SetConsoleCtrlHandler(
            handler: Option<unsafe extern "system" fn(u32) -> i32>,
            add: i32,
        ) -> i32;
    }
    let state = &*JET_INTERRUPT_STATE;
    let _lock = state
        .install_lock
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if state
        .installed
        .load(std::sync::atomic::Ordering::Acquire)
        & JET_SIGNAL_INT
        != 0
    {
        return Ok(());
    }
    // A parent may have disabled Ctrl-C with the documented NULL handler;
    // clear that inherited process flag before installing Jet's handler.
    unsafe { SetConsoleCtrlHandler(None, 0) };
    let installed = unsafe { SetConsoleCtrlHandler(Some(jet_interrupt_mark), 1) };
    if installed == 0 {
        return Err("could not install the Windows console Ctrl-C handler".to_string());
    }
    state
        .installed
        .store(JET_SIGNAL_INT, std::sync::atomic::Ordering::Release);
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn jet_interrupt_arm_mask(_: usize) -> Result<(), String> {
    Err(jet_interrupt_unavailable_error().to_string())
}

/// Arm the process for Ctrl-C interrupts. Called by a tier's first
/// `core.sys.on_interrupt` registration; idempotent and shared.
pub fn jet_interrupt_arm() -> Result<(), String> {
    jet_interrupt_arm_mask(JET_SIGNAL_INT)
}

static JET_PROCESS_SIGNAL_DISPATCHER: std::sync::LazyLock<Result<(), String>> =
    std::sync::LazyLock::new(|| {
        std::thread::Builder::new()
            .name("jet-process-signal".to_string())
            .spawn(|| loop {
                std::thread::sleep(jet_interrupt_poll_interval());
                jet_process_signal_dispatch();
            })
            .map(|_| ())
            .map_err(jet_interrupt_dispatcher_start_error)
    });

fn jet_process_signal_dispatcher_start() -> Result<(), String> {
    match &*JET_PROCESS_SIGNAL_DISPATCHER {
        Ok(()) => Ok(()),
        Err(message) => Err(message.clone()),
    }
}

/// Register the cancellation callback for one typed process signal. The
/// callback slot is replaced, not accumulated: one signal has one root-task
/// meaning and resident runs must not retain old callbacks.
pub fn jet_process_signal_register(
    mask: usize,
    callback: JetProcessSignalCallback,
) -> Result<(), String> {
    let Some(index) = jet_process_signal_index(mask) else {
        return Err("invalid process signal".to_string());
    };
    jet_interrupt_arm_mask(mask)?;
    jet_process_signal_dispatcher_start()?;
    JET_INTERRUPT_STATE
        .process_handlers
        .lock()
        .unwrap_or_else(|error| error.into_inner())[index] = Some(callback);
    Ok(())
}

fn jet_process_signal_dispatch() {
    let state = &*JET_INTERRUPT_STATE;
    let _dispatch_lock = state
        .process_dispatch_lock
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let callbacks = loop {
        let pending = state
            .pending_process_signals
            .load(std::sync::atomic::Ordering::Acquire);
        if pending == 0 {
            return;
        }
        let (registered, callbacks) = {
            let handlers = state
                .process_handlers
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let mut registered = 0;
            let mut callbacks = Vec::new();
            for (bit, handler) in [
                (JET_SIGNAL_TERM, handlers[2].as_ref()),
                (JET_SIGNAL_HUP, handlers[1].as_ref()),
                (JET_SIGNAL_INT, handlers[0].as_ref()),
            ] {
                if pending & bit != 0 {
                    if let Some(handler) = handler {
                        registered |= bit;
                        callbacks.push(handler.clone());
                    }
                }
            }
            (registered, callbacks)
        };
        let deliverable = pending & registered;
        if deliverable == 0 {
            // Keep an event for a later registration, matching the empty
            // `jet_interrupt_dispatch` rule.
            return;
        }
        if state
            .pending_process_signals
            .compare_exchange(
                pending,
                pending & !deliverable,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .is_ok()
        {
            break callbacks;
        }
    };
    for callback in callbacks {
        callback();
    }
}

/// Drop anything pending. A run boundary starts with no signal, so a signal
/// marked for a previous resident run never lands on the next one.
pub fn jet_interrupt_clear() {
    let state = &*JET_INTERRUPT_STATE;
    let _dispatch_lock = state
        .process_dispatch_lock
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    state
        .pending_interrupts
        .store(0, std::sync::atomic::Ordering::Release);
    state
        .pending_process_signals
        .store(0, std::sync::atomic::Ordering::Release);
}

/// The one Ctrl-C consumption rule: take the whole pending count, then run
/// every registered handler once per counted interrupt, in registration order.
///
/// An empty `handlers` slice does NOT consume. The count is process-wide and
/// shared, so a drain with nothing registered must leave the signal for the
/// drain that can actually deliver it.
pub fn jet_interrupt_dispatch<T>(handlers: &[T], mut invoke: impl FnMut(&T)) {
    if handlers.is_empty() {
        return;
    }
    let count = JET_INTERRUPT_STATE
        .pending_interrupts
        .swap(0, std::sync::atomic::Ordering::Acquire);
    for _ in 0..count {
        for handler in handlers {
            invoke(handler);
        }
    }
}

#[cfg(test)]
mod jet_interrupt_tests {
    use super::*;

    /// Serialise the tests below: they share the one process count, which is
    /// the property under test.
    fn with_queue<R>(body: impl FnOnce() -> R) -> R {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = LOCK.lock().unwrap_or_else(|error| error.into_inner());
        jet_interrupt_clear();
        let result = body();
        jet_interrupt_clear();
        result
    }

    fn note() {
        JET_INTERRUPT_STATE
            .pending_interrupts
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    #[test]
    fn dispatch_is_count_first_then_registration_order() {
        with_queue(|| {
            note();
            note();
            let mut seen = Vec::new();
            jet_interrupt_dispatch(&['a', 'b'], |handler| seen.push(*handler));
            assert_eq!(seen, vec!['a', 'b', 'a', 'b']);
        });
    }

    #[test]
    fn a_drain_with_no_handlers_leaves_the_interrupt_for_the_tier_that_has_one() {
        with_queue(|| {
            note();
            let empty: [char; 0] = [];
            jet_interrupt_dispatch(&empty, |_| panic!("an empty drain must not invoke"));
            let mut seen = Vec::new();
            jet_interrupt_dispatch(&['a'], |handler| seen.push(*handler));
            assert_eq!(
                seen,
                vec!['a'],
                "an empty drain stole the one process interrupt count"
            );
        });
    }

    #[test]
    fn clear_drops_a_previous_runs_interrupt() {
        with_queue(|| {
            note();
            jet_interrupt_clear();
            jet_interrupt_dispatch(&['a'], |_| panic!("a cleared interrupt was delivered"));
        });
    }
}
