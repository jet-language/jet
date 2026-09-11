// Portable GC adapter.  This file is emitted only with the checked no-OS
// Prelude.  It uses alloc/core only: a target-sized spin lock, a one-time
// cell, and a target-sized identity counter.  Collection itself remains in
// __gc_core.rs and is never replaced by a no-op path.

extern crate alloc;

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use super::{Fault, GcPanic, ObjectId, PromotionSite};

pub(crate) struct GcMutex<T> {
    locked: AtomicBool,
    value: UnsafeCell<T>,
}

unsafe impl<T: Send> Send for GcMutex<T> {}
unsafe impl<T: Send> Sync for GcMutex<T> {}

pub(crate) struct GcMutexGuard<'a, T> {
    mutex: &'a GcMutex<T>,
}

impl<T> Deref for GcMutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // The guard owns the lock until Drop, so this shared reference cannot
        // race with a mutable access through another guard.
        unsafe { &*self.mutex.value.get() }
    }
}

impl<T> DerefMut for GcMutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // The guard is the sole owner of the lock while this mutable borrow is
        // alive.
        unsafe { &mut *self.mutex.value.get() }
    }
}

impl<T> Drop for GcMutexGuard<'_, T> {
    fn drop(&mut self) {
        self.mutex.locked.store(false, Ordering::Release);
    }
}

pub(crate) struct GcPoisonError<T>(T);

impl<T> GcPoisonError<T> {
    pub(crate) fn into_inner(self) -> T {
        self.0
    }
}

pub(crate) enum GcTryLockError<T> {
    WouldBlock,
    Poisoned(T),
}

impl<T> GcMutex<T> {
    pub(crate) const fn new(value: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            value: UnsafeCell::new(value),
        }
    }

    pub(crate) fn into_inner(self) -> Result<T, GcPoisonError<T>> {
        Ok(self.value.into_inner())
    }

    #[inline]
    fn acquire(&self) {
        while self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
    }

    pub(crate) fn lock(&self) -> Result<GcMutexGuard<'_, T>, GcPoisonError<GcMutexGuard<'_, T>>> {
        self.acquire();
        Ok(GcMutexGuard { mutex: self })
    }

    pub(crate) fn try_lock(&self) -> Result<GcMutexGuard<'_, T>, GcTryLockError<GcMutexGuard<'_, T>>> {
        if self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            Ok(GcMutexGuard { mutex: self })
        } else {
            Err(GcTryLockError::WouldBlock)
        }
    }

    pub(crate) fn get_mut(
        &mut self,
    ) -> Result<&mut T, GcPoisonError<&mut T>> {
        // An exclusive borrow of the mutex proves no guard can be active.
        Ok(self.value.get_mut())
    }
}

pub(crate) struct GcOnce<T> {
    state: AtomicU8,
    value: UnsafeCell<MaybeUninit<T>>,
}

unsafe impl<T: Send + Sync> Sync for GcOnce<T> {}
unsafe impl<T: Send> Send for GcOnce<T> {}

impl<T> GcOnce<T> {
    pub(crate) const fn new() -> Self {
        Self {
            state: AtomicU8::new(0),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    pub(crate) fn get_or_init(&'static self, init: impl FnOnce() -> T) -> &'static T {
        if self.state.load(Ordering::Acquire) != 2
            && self
                .state
                .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
        {
            // A no-OS panic policy aborts, so an initializer cannot return
            // while the cell is in the initializing state.
            unsafe { (*self.value.get()).write(init()) };
            self.state.store(2, Ordering::Release);
        } else {
            while self.state.load(Ordering::Acquire) != 2 {
                core::hint::spin_loop();
            }
        }
        unsafe { (*self.value.get()).assume_init_ref() }
    }
}

static NEXT_OBJECT_ID: GcMutex<u64> = GcMutex::new(1);

pub(crate) fn gc_next_object_id() -> Result<u64, Fault> {
    let mut next = NEXT_OBJECT_ID.lock().map_err(|_| Fault::HeapPoisoned)?;
    let id = *next;
    *next = next.checked_add(1).ok_or(Fault::IdExhausted)?;
    Ok(id)
}

pub(crate) fn gc_catch_unwind<F, R>(f: F) -> Result<R, GcPanic>
where
    F: FnOnce() -> R,
{
    Ok(f())
}

pub(crate) fn gc_resume_unwind(_payload: GcPanic) -> ! {
    crate::jet_target_exit(101)
}

pub(crate) fn gc_initialize_trace() -> Result<(), Fault> {
    // Trace is intentionally inactive without a hosted filesystem/env/time
    // adapter.  It does not alter allocation or collection behavior.
    Ok(())
}

pub(crate) fn gc_runtime_or_exit(fault: Fault) -> ! {
    crate::jet_target_failure_render(1, |out| super::gc_write_failure(out, &fault))
}

pub(crate) fn gc_record_memory_ledger(_site: PromotionSite) {
    // The portable closure has no configured hosted memory-ledger provider.
}

pub(crate) fn gc_trace_promotion(_id: ObjectId, _site: PromotionSite) -> Result<(), Fault> {
    Ok(())
}

pub(crate) fn gc_trace_collection(_reclaimed: &[ObjectId]) -> Result<(), Fault> {
    Ok(())
}
