/// The one signal-handler mechanism (#2027, I8 + I9).
///
/// This file is the single home for every interrupt fact: the process pending
/// count, the platform handler that increments it, the one-shot arm path, the
/// count-first / registration-order consumption rule, and the refusal texts.
/// Nothing outside this file may declare a second count, a second mark
/// function, or a second install path.
///
/// Three tiers reach it and none of them restates any of it:
///   * AOT — `Prelude/CoreLib/Top/FSIoEnvOsTesting.rs`'s `jet_os_interrupt`,
///     which the generated program embeds together with this source
///     (`Codegen/mod.rs` `needs_interrupt`);
///   * the resident Cranelift host — `jet-jit/src/CoreHost.rs`, via
///     `jet_codegen::interrupt_runtime`;
///   * the TIR evaluator ambient — `Codegen/TIR/eval/mod.rs`, via
///     `crate::interrupt_runtime`.
///
/// The last two share ONE compiled instance (`jet-codegen/src/lib.rs`,
/// `pub mod interrupt_runtime`), so the `jet` binary has one pending count, one
/// SIGINT disposition, and one arm result no matter which tier is running. That
/// is the whole point: `signal(SIGINT, …)` REPLACES the process handler, so a
/// second instance did not merely duplicate state — it silently disarmed the
/// first tier's queue forever.
///
/// A tier supplies only its handler storage and its invocation adapter, because
/// an AOT `Arc<dyn Fn>`, a JIT `(code address, environment)` pair and an
/// evaluator callable index cannot be one Rust value. Everything a tier could
/// disagree about is decided here instead.
///
/// `#Shield` never enters this file, by construction. A shield defers a
/// *cooperative* interrupt — a cancel or a blown deadline — at the wait points
/// of the shielded task (`Prelude/Scheduler.rs::jet_scheduler_shielded`). An OS
/// signal is not a wait-point outcome: the platform handler only increments the
/// count below, and `jet_interrupt_dispatch` reads no task control, no
/// `SHIELD_DEPTH`, and no unwind state. So a signal delivered while a task is
/// inside `#Shield { … }` is neither deferred to the region's exit nor
/// discarded: it is counted, and the handlers run on the next drain while the
/// shielded region keeps running. All three tiers answer that identically
/// because they call this one `jet_interrupt_dispatch`, which has no way to
/// consult shield state — not because three copies happen to agree.
pub fn jet_interrupt_poll_interval() -> std::time::Duration {
    std::time::Duration::from_millis(10)
}

pub fn jet_interrupt_core_error(message: &str) -> String {
    format!("core.sys.on_interrupt: {message}")
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

/// The one pending-interrupt count. There is no queue *type*, so no tier can
/// instantiate a second one — the previous `JetInterruptQueue` struct existed
/// only to be declared three times.
static JET_INTERRUPT_PENDING: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// The one arm result. `signal`/`SetConsoleCtrlHandler` are process-wide
/// replacements, so arming is once per process and every tier reads the same
/// outcome. The initializer lives with the static, so there is no second way to
/// arm and no unarmed-but-registered state to reason about.
static JET_INTERRUPT_ARMED: std::sync::LazyLock<Result<(), String>> =
    std::sync::LazyLock::new(jet_interrupt_install);

/// The platform callback. It does no allocation, no locking, and no user work —
/// one relaxed increment is the whole async-signal-safe body.
#[cfg(unix)]
extern "C" fn jet_interrupt_mark(_: i32) {
    JET_INTERRUPT_PENDING.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(windows)]
unsafe extern "system" fn jet_interrupt_mark(kind: u32) -> i32 {
    const CTRL_C_EVENT: u32 = 0;
    if kind == CTRL_C_EVENT {
        JET_INTERRUPT_PENDING.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        1
    } else {
        0
    }
}

#[cfg(unix)]
fn jet_interrupt_install() -> Result<(), String> {
    extern "C" {
        fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
    }
    const SIGINT: i32 = 2;
    let previous = unsafe { signal(SIGINT, jet_interrupt_mark) };
    if previous == usize::MAX {
        Err("could not install the SIGINT handler".to_string())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn jet_interrupt_install() -> Result<(), String> {
    extern "system" {
        fn SetConsoleCtrlHandler(
            handler: Option<unsafe extern "system" fn(u32) -> i32>,
            add: i32,
        ) -> i32;
    }
    // A parent may have disabled Ctrl-C with the documented NULL handler;
    // clear that inherited process flag before installing Jet's handler.
    unsafe { SetConsoleCtrlHandler(None, 0) };
    let installed = unsafe { SetConsoleCtrlHandler(Some(jet_interrupt_mark), 1) };
    if installed == 0 {
        Err("could not install the Windows console Ctrl-C handler".to_string())
    } else {
        Ok(())
    }
}

#[cfg(not(any(unix, windows)))]
fn jet_interrupt_install() -> Result<(), String> {
    Err(jet_interrupt_unavailable_error().to_string())
}

/// Arm the process for interrupts. Called by a tier's first
/// `core.sys.on_interrupt` registration; idempotent and shared.
pub fn jet_interrupt_arm() -> Result<(), String> {
    match &*JET_INTERRUPT_ARMED {
        Ok(()) => Ok(()),
        Err(message) => Err(message.clone()),
    }
}

/// Drop anything pending. A run boundary starts with no interrupt, so a signal
/// marked for a previous dev/restart instance never lands on the next one.
pub fn jet_interrupt_clear() {
    JET_INTERRUPT_PENDING.store(0, std::sync::atomic::Ordering::Release);
}

/// The one consumption rule: take the whole pending count, then run every
/// registered handler once per counted interrupt, in registration order.
///
/// An empty `handlers` slice does NOT consume. The count is process-wide and
/// shared, so a drain with nothing registered — a tier polling before its first
/// registration, or a tier that is not the one running the program — must leave
/// a delivered signal for the drain that can actually deliver it.
///
/// `invoke` MUST end whatever control transfer its handler raises; a tier that
/// merely catches one loses it. The payload is the one thing a tier cannot
/// share — an AOT unwind, a resident-host trap, an evaluator diagnostic — so
/// each ends it in its own adapter: AOT at `Prelude/Core.rs`'s
/// `jet_interrupt_handler_unwind` (an explicit `process.exit` still exits, a
/// stop reports and this drain continues to the next handler), the resident
/// host by setting a runtime trap, the evaluator by returning the diagnostic.
/// Dropping it silently is what made a signalled AOT program run forever with
/// its handlers already done.
pub fn jet_interrupt_dispatch<T>(handlers: &[T], mut invoke: impl FnMut(&T)) {
    if handlers.is_empty() {
        return;
    }
    let count = JET_INTERRUPT_PENDING.swap(0, std::sync::atomic::Ordering::Acquire);
    for _ in 0..count {
        for handler in handlers {
            invoke(handler);
        }
    }
}

#[cfg(test)]
mod jet_interrupt_tests {
    use super::*;

    /// Serialise the tests below: they share the one process count, which is the
    /// property under test.
    fn with_queue<R>(body: impl FnOnce() -> R) -> R {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = LOCK.lock().unwrap_or_else(|error| error.into_inner());
        jet_interrupt_clear();
        let result = body();
        jet_interrupt_clear();
        result
    }

    fn note() {
        JET_INTERRUPT_PENDING.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
