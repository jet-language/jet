/// Shared interrupt pending/ordering semantics.
///
/// Platform signal handlers only call `note`. Each execution tier owns the
/// callback storage and invocation adapter, but all tiers consume pending
/// interrupts with the same count-first, registration-order rule.
pub(crate) fn jet_interrupt_poll_interval() -> std::time::Duration {
    std::time::Duration::from_millis(10)
}

pub(crate) fn jet_interrupt_core_error(message: &str) -> String {
    format!("core.os.on_interrupt: {message}")
}

pub(crate) fn jet_interrupt_dispatcher_start_error(error: impl std::fmt::Display) -> String {
    format!("could not start interrupt dispatcher: {error}")
}

pub(crate) fn jet_interrupt_dispatcher_stopped_error() -> &'static str {
    "interrupt dispatcher stopped"
}

pub(crate) fn jet_interrupt_invalid_callback_record_error() -> &'static str {
    "invalid interrupt callback record"
}

pub(crate) fn jet_interrupt_invalid_callback_value_error() -> &'static str {
    "core.os.on_interrupt callback"
}

pub(crate) fn jet_interrupt_unavailable_error() -> &'static str {
    "interrupt handling is unavailable on this target"
}

#[cfg(unix)]
pub(crate) fn jet_interrupt_install_unix_handler(
    handler: extern "C" fn(i32),
) -> Result<(), String> {
    extern "C" {
        fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
    }
    const SIGINT: i32 = 2;
    let previous = unsafe { signal(SIGINT, handler) };
    if previous == usize::MAX {
        Err("could not install the SIGINT handler".to_string())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
pub(crate) fn jet_interrupt_install_windows_handler(
    handler: Option<unsafe extern "system" fn(u32) -> i32>,
) -> Result<(), String> {
    extern "system" {
        fn SetConsoleCtrlHandler(
            handler: Option<unsafe extern "system" fn(u32) -> i32>,
            add: i32,
        ) -> i32;
    }
    // A parent may have disabled Ctrl-C with the documented NULL handler;
    // clear that inherited process flag before installing Jet's handler.
    unsafe { SetConsoleCtrlHandler(None, 0) };
    let installed = unsafe { SetConsoleCtrlHandler(handler, 1) };
    if installed == 0 {
        Err("could not install the Windows console Ctrl-C handler".to_string())
    } else {
        Ok(())
    }
}

pub(crate) struct JetInterruptQueue {
    pending: std::sync::atomic::AtomicUsize,
}

impl JetInterruptQueue {
    pub(crate) const fn new() -> Self {
        Self {
            pending: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    pub(crate) fn note(&self) {
        self.pending
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn clear(&self) {
        self.pending
            .store(0, std::sync::atomic::Ordering::Release);
    }

    pub(crate) fn dispatch<T>(&self, handlers: &[T], mut invoke: impl FnMut(&T)) {
        let count = self
            .pending
            .swap(0, std::sync::atomic::Ordering::Acquire);
        for _ in 0..count {
            for handler in handlers {
                invoke(handler);
            }
        }
    }
}
