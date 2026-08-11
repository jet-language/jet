/// Shared interrupt pending/ordering semantics.
///
/// Platform signal handlers only call `note`. Each execution tier owns the
/// callback storage and invocation adapter, but all tiers consume pending
/// interrupts with the same count-first, registration-order rule.
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
