//! Native memory carriers for the resident Cranelift runtime.

use super::Concurrency;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering, compiler_fence};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

static SHARED_TRANSACTION_SERIAL: Mutex<()> = Mutex::new(());

thread_local! {
    static SHARED_TRANSACTIONS: std::cell::RefCell<Vec<SharedTransaction>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

struct SharedTransaction {
    entries: Vec<SharedTransactionEntry>,
    _serial: Option<MutexGuard<'static, ()>>,
}

struct SharedTransactionEntry {
    handle: i64,
    shared: Arc<SharedState>,
    staged: i64,
    record: bool,
}

struct SharedLockGuard(Vec<Arc<SharedState>>);

impl SharedLockGuard {
    fn acquire(entries: &[SharedTransactionEntry]) -> Self {
        let mut locked = Vec::with_capacity(entries.len());
        for entry in entries {
            entry.shared.lock();
            locked.push(Arc::clone(&entry.shared));
        }
        Self(locked)
    }
}

impl Drop for SharedLockGuard {
    fn drop(&mut self) {
        for shared in self.0.iter().rev() {
            shared.unlock();
        }
    }
}

#[derive(Default)]
pub(crate) struct AllocatorState {
    generation: u64,
}

#[derive(Clone, Copy)]
struct PoolSlot {
    generation: u32,
    value: Option<i64>,
}

#[derive(Default)]
pub(crate) struct PoolState {
    slots: Vec<PoolSlot>,
}

pub(crate) struct SharedState {
    locked: AtomicBool,
    value: AtomicI64,
}

pub(crate) struct ConditionState {
    lock: Mutex<()>,
    wake: Condvar,
}

impl ConditionState {
    fn new() -> Self {
        Self {
            lock: Mutex::new(()),
            wake: Condvar::new(),
        }
    }

    fn notify_one(&self) {
        self.wake.notify_one();
    }

    fn notify_all(&self) {
        self.wake.notify_all();
    }

    fn wait_once(&self) {
        let guard = self.lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let _ = self
            .wake
            .wait_timeout(guard, Duration::from_millis(10))
            .unwrap_or_else(|poisoned| poisoned.into_inner());
    }
}

pub(crate) struct ExpiringState {
    value: i64,
    expires_at: i64,
    clock: i64,
    secret: Option<SecretState>,
}

pub(crate) struct SecretState {
    handle: i64,
    bytes: Vec<u8>,
}

impl SecretState {
    pub(crate) fn from_material(handle: i64, bytes: Vec<u8>) -> Self {
        Self { handle, bytes }
    }

    fn zeroize(&mut self) {
        for byte in self.bytes.iter_mut() {
            // SAFETY: the pointer refers to this live, uniquely borrowed byte.
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
        compiler_fence(Ordering::SeqCst);
    }
}

impl Drop for SecretState {
    fn drop(&mut self) {
        self.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::SecretState;

    #[test]
    fn secret_storage_zeroizes_the_owned_bytes() {
        let mut secret = SecretState::from_material(1, vec![0x5a; 32]);
        secret.zeroize();
        assert_eq!(secret.bytes.as_slice(), &[0; 32]);
    }
}

impl SharedState {
    fn new(value: i64) -> Self {
        Self {
            locked: AtomicBool::new(false),
            value: AtomicI64::new(value),
        }
    }

    fn lock(&self) {
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            std::thread::yield_now();
        }
    }

    fn unlock(&self) {
        self.locked.store(false, Ordering::Release);
    }
}

fn pool(rt: &crate::JitRuntime, handle: i64) -> Option<Arc<Mutex<PoolState>>> {
    rt.pools.get((handle as usize).wrapping_sub(1)).cloned()
}

fn shared(rt: &crate::JitRuntime, handle: i64) -> Option<Arc<SharedState>> {
    rt.shareds.get((handle as usize).wrapping_sub(1)).cloned()
}

fn condition(rt: &crate::JitRuntime, handle: i64) -> Option<Arc<ConditionState>> {
    rt.conditions.get((handle as usize).wrapping_sub(1)).cloned()
}

const GUARD_SHARED: i64 = 0;
const GUARD_VALUE: i64 = 1;
const GUARD_EDITABLE: i64 = 2;

fn guard_shared_handle(rt: &crate::JitRuntime, guard: i64) -> Option<i64> {
    let handle = rt.heap.record_get_int(guard, GUARD_SHARED)?;
    (handle != 0).then_some(handle)
}

fn pack_shared_guard(rt: &mut crate::JitRuntime, shared_handle: i64, value: i64, editable: i64) -> i64 {
    let guard = rt.heap.alloc_record(3);
    let _ = rt.heap.record_set_int(guard, GUARD_SHARED, shared_handle);
    let _ = rt.heap.record_set_int(guard, GUARD_VALUE, value);
    let _ = rt.heap.record_set_int(guard, GUARD_EDITABLE, editable);
    guard
}

fn pack_id(index: usize, generation: u32) -> i64 {
    (i64::from(generation) << 32) | (index as i64 + 1)
}

fn unpack_id(id: i64) -> Option<(usize, u32)> {
    let low = (id as u64 & 0xffff_ffff) as u32;
    (low != 0).then_some(((low - 1) as usize, (id as u64 >> 32) as u32))
}

extern "C" fn jet_jit_allocator_new() -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.allocators.push(AllocatorState::default());
        rt.allocators.len() as i64
    })
}

extern "C" fn jet_jit_allocator_alloc(handle: i64, value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(state) = rt.allocators.get_mut((handle as usize).wrapping_sub(1)) else {
            rt.set_trap("allocator handle is closed or invalid");
            return 0;
        };
        let _ = state.generation;
        value
    })
}

extern "C" fn jet_jit_allocator_reset(handle: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let Some(state) = rt.allocators.get_mut((handle as usize).wrapping_sub(1)) else {
            rt.set_trap("allocator handle is closed or invalid");
            return;
        };
        state.generation = state.generation.wrapping_add(1);
    });
}

extern "C" fn jet_jit_pool_new() -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.pools.push(Arc::new(Mutex::new(PoolState::default())));
        rt.pools.len() as i64
    })
}

extern "C" fn jet_jit_pool_add(handle: i64, value: i64) -> i64 {
    let Some(pool) = Concurrency::with_runtime_mut(|rt| pool(rt, handle)) else {
        return 0;
    };
    let mut pool = pool.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((index, slot)) = pool
        .slots
        .iter_mut()
        .enumerate()
        .find(|(_, slot)| slot.value.is_none())
    {
        slot.value = Some(value);
        return pack_id(index, slot.generation);
    }
    let index = pool.slots.len();
    pool.slots.push(PoolSlot {
        generation: 0,
        value: Some(value),
    });
    pack_id(index, 0)
}

fn pool_value(handle: i64, id: i64) -> Option<i64> {
    let pool = Concurrency::with_runtime_mut(|rt| pool(rt, handle))?;
    let pool = pool.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let (index, generation) = unpack_id(id)?;
    let slot = pool.slots.get(index)?;
    (slot.generation == generation).then_some(slot.value).flatten()
}

extern "C" fn jet_jit_pool_get(handle: i64, id: i64) -> i64 {
    match pool_value(handle, id) {
        Some(value) => value,
        None => {
            Concurrency::with_runtime_mut(|rt| {
                rt.set_trap(
                    "this Id no longer refers to a live value — its pool slot was removed",
                )
            });
            0
        }
    }
}

extern "C" fn jet_jit_pool_remove(handle: i64, id: i64) -> i64 {
    let Some(pool) = Concurrency::with_runtime_mut(|rt| pool(rt, handle)) else {
        return 0;
    };
    let mut pool = pool.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some((index, generation)) = unpack_id(id) else {
        return 0;
    };
    let Some(slot) = pool.slots.get_mut(index) else {
        return 0;
    };
    if slot.generation != generation {
        return 0;
    }
    let Some(value) = slot.value.take() else {
        return 0;
    };
    slot.generation = slot.generation.wrapping_add(1);
    Concurrency::with_runtime_mut(|rt| {
        crate::runtime_host::alloc_jit_result(rt, true, value as u64)
    })
}

extern "C" fn jet_jit_pool_ids(handle: i64) -> i64 {
    let Some(pool) = Concurrency::with_runtime_mut(|rt| pool(rt, handle)) else {
        return 0;
    };
    let ids = {
        let pool = pool.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        pool.slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| {
                slot.value
                    .is_some()
                    .then_some(pack_id(index, slot.generation))
            })
            .collect::<Vec<_>>()
    };
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_int_list(ids))
}

extern "C" fn jet_jit_shared_new(value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.shareds.push(Arc::new(SharedState::new(value)));
        rt.shareds.len() as i64
    })
}

extern "C" fn jet_jit_shared_begin(handle: i64) -> i64 {
    let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, handle)) else {
        return 0;
    };
    shared.lock();
    shared.value.load(Ordering::Relaxed)
}

extern "C" fn jet_jit_shared_end_read(handle: i64) {
    if let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, handle)) {
        shared.unlock();
    }
}

extern "C" fn jet_jit_shared_end_write(handle: i64, value: i64) {
    if let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, handle)) {
        shared.value.store(value, Ordering::Relaxed);
        shared.unlock();
    }
}

/// D-SHARED-CYCLE1=C: weak handle is the same slot index; upgrade packs
/// Option as `0` (None) or `handle + 1` (Some).
extern "C" fn jet_jit_shared_downgrade(handle: i64) -> i64 {
    handle
}

extern "C" fn jet_jit_shared_strong_count(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        shared(rt, handle)
            .map(|state| Arc::strong_count(&state) as i64)
            .unwrap_or(0)
    })
}

extern "C" fn jet_jit_shared_weak_upgrade(weak: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        if shared(rt, weak).is_some() {
            weak.wrapping_add(1)
        } else {
            0
        }
    })
}

extern "C" fn jet_jit_condition_new() -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.conditions.push(Arc::new(ConditionState::new()));
        rt.conditions.len() as i64
    })
}

extern "C" fn jet_jit_condition_notify_one(handle: i64) {
    if let Some(condition) = Concurrency::with_runtime_mut(|rt| condition(rt, handle)) {
        condition.notify_one();
    }
}

extern "C" fn jet_jit_condition_notify_all(handle: i64) {
    if let Some(condition) = Concurrency::with_runtime_mut(|rt| condition(rt, handle)) {
        condition.notify_all();
    }
}

extern "C" fn jet_jit_shared_guard_begin(handle: i64, editable: i64) -> i64 {
    let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, handle)) else {
        return 0;
    };
    shared.lock();
    let value = shared.value.load(Ordering::Relaxed);
    Concurrency::with_runtime_mut(|rt| pack_shared_guard(rt, handle, value, i64::from(editable != 0)))
}

extern "C" fn jet_jit_shared_guard_value(guard: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.record_get_int(guard, GUARD_VALUE).unwrap_or(0))
}

extern "C" fn jet_jit_shared_guard_set_value(guard: i64, value: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let _ = rt.heap.record_set_int(guard, GUARD_VALUE, value);
        let Some(shared_handle) = guard_shared_handle(rt, guard) else {
            return;
        };
        if rt.heap.record_get_int(guard, GUARD_EDITABLE).unwrap_or(0) == 0 {
            return;
        };
        if let Some(shared) = shared(rt, shared_handle) {
            shared.value.store(value, Ordering::Relaxed);
        }
    });
}

extern "C" fn jet_jit_shared_guard_end(guard: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let Some(shared_handle) = guard_shared_handle(rt, guard) else {
            return;
        };
        let value = rt.heap.record_get_int(guard, GUARD_VALUE).unwrap_or(0);
        let editable = rt.heap.record_get_int(guard, GUARD_EDITABLE).unwrap_or(0) != 0;
        let _ = rt.heap.record_set_int(guard, GUARD_SHARED, 0);
        if let Some(shared) = shared(rt, shared_handle) {
            if editable {
                shared.value.store(value, Ordering::Relaxed);
            }
            shared.unlock();
        }
    });
}

extern "C" fn jet_jit_shared_guard_wait_once(guard: i64, condition_handle: i64) {
    let Some((shared_handle, condition)) = Concurrency::with_runtime_mut(|rt| {
        let shared_handle = guard_shared_handle(rt, guard)?;
        let condition = condition(rt, condition_handle)?;
        Some((shared_handle, condition))
    }) else {
        Concurrency::with_runtime_mut(|rt| {
            rt.set_trap("SharedGuard wait on an invalid or released guard");
        });
        return;
    };
    let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, shared_handle)) else {
        Concurrency::with_runtime_mut(|rt| {
            rt.set_trap("SharedGuard wait: shared handle is closed or invalid");
        });
        return;
    };
    shared.unlock();
    condition.wait_once();
    shared.lock();
    let fresh = shared.value.load(Ordering::Relaxed);
    Concurrency::with_runtime_mut(|rt| {
        let _ = rt.heap.record_set_int(guard, GUARD_VALUE, fresh);
    });
}

extern "C" fn jet_jit_shared_txn_begin() {
    SHARED_TRANSACTIONS.with(|transactions| {
        let serial = transactions.borrow().is_empty().then(|| {
            // ponytail: replace this global serialization only when measured
            // transaction throughput justifies a versioned retry protocol.
            SHARED_TRANSACTION_SERIAL
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
        });
        transactions.borrow_mut().push(SharedTransaction {
            entries: Vec::new(),
            _serial: serial,
        });
    });
}

extern "C" fn jet_jit_shared_txn_get(handle: i64) -> i64 {
    if let Some(staged) = SHARED_TRANSACTIONS.with(|transactions| {
        transactions
            .borrow()
            .last()
            .and_then(|transaction| {
                transaction
                    .entries
                    .iter()
                    .find(|entry| entry.handle == handle)
            })
            .map(|entry| entry.staged)
    }) {
        return staged;
    }
    let Some((shared, original, staged, record)) = Concurrency::with_runtime_mut(|rt| {
        let shared = shared(rt, handle)?;
        let original = shared.value.load(Ordering::Acquire);
        let staged = rt.heap.alloc_record(0);
        let record = rt.heap.record_assign_from(staged, original).is_some();
        Some((shared, original, if record { staged } else { original }, record))
    }) else {
        return 0;
    };
    SHARED_TRANSACTIONS.with(|transactions| {
        let mut transactions = transactions.borrow_mut();
        let Some(transaction) = transactions.last_mut() else {
            return 0;
        };
        transaction.entries.push(SharedTransactionEntry {
            handle,
            shared,
            staged,
            record,
        });
        staged
    })
}

extern "C" fn jet_jit_shared_txn_set(handle: i64, value: i64) {
    SHARED_TRANSACTIONS.with(|transactions| {
        if let Some(entry) = transactions
            .borrow_mut()
            .last_mut()
            .and_then(|transaction| {
                transaction
                    .entries
                    .iter_mut()
                    .find(|entry| entry.handle == handle)
            })
        {
            entry.staged = value;
        }
    });
}

extern "C" fn jet_jit_shared_txn_commit() {
    let Some(mut transaction) =
        SHARED_TRANSACTIONS.with(|transactions| transactions.borrow_mut().pop())
    else {
        return;
    };
    transaction.entries.sort_by_key(|entry| entry.handle);
    let _locks = SharedLockGuard::acquire(&transaction.entries);
    Concurrency::with_runtime_mut(|rt| {
        for entry in &transaction.entries {
            if entry.record {
                let current = entry.shared.value.load(Ordering::Acquire);
                if rt
                    .heap
                    .record_assign_from(current, entry.staged)
                    .is_none()
                {
                    rt.set_trap("Shared transaction record payload became invalid");
                    return;
                }
            } else {
                entry.shared.value.store(entry.staged, Ordering::Release);
            }
        }
    });
}

extern "C" fn jet_jit_shared_txn_abort() {
    SHARED_TRANSACTIONS.with(|transactions| {
        transactions.borrow_mut().pop();
    });
}

extern "C" fn jet_jit_expiring_new(
    value: i64,
    duration: i64,
    clock: i64,
    secret: i64,
) -> i64 {
    // SigningKey / X25519 / Secret live in crypto_values (#1222). Claim a
    // zeroize mirror here; keep the crypto handle live for `with` loans.
    let owned_secret = if secret != 0 {
        match crate::Crypto::claim_expiring_secret(value) {
            Some(state) => Some(state),
            None => {
                Concurrency::with_runtime_mut(|rt| {
                    rt.set_trap("secret key handle is invalid or already moved");
                });
                return 0;
            }
        }
    } else {
        None
    };
    Concurrency::with_runtime_mut(|rt| {
        let now = rt
            .clocks
            .get((clock as usize).wrapping_sub(1))
            .copied()
            .unwrap_or(0);
        rt.expirings.push(ExpiringState {
            value,
            expires_at: now.saturating_add(duration.max(0)),
            clock,
            secret: owned_secret,
        });
        rt.expirings.len() as i64
    })
}

extern "C" fn jet_jit_expiring_get(handle: i64, clock: i64) -> i64 {
    let (status, drop_crypto) = Concurrency::with_runtime_mut(|rt| {
        let stored_clock = rt
            .expirings
            .get((handle as usize).wrapping_sub(1))
            .map(|value| value.clock)
            .unwrap_or(0);
        let clock = if clock == 0 { stored_clock } else { clock };
        let now = rt
            .clocks
            .get((clock as usize).wrapping_sub(1))
            .copied()
            .unwrap_or(0);
        let Some(value) = rt.expirings.get_mut((handle as usize).wrapping_sub(1)) else {
            return (0_i64, None);
        };
        if now > value.expires_at {
            let crypto_handle = value.value;
            value.secret.take();
            value.value = 0;
            return (0, Some(crypto_handle));
        }
        (value.value + 1, None)
    });
    if let Some(crypto_handle) = drop_crypto {
        crate::Crypto::drop_crypto_handle(crypto_handle);
    }
    status
}

extern "C" fn jet_jit_expiring_is_valid(handle: i64, clock: i64) -> i8 {
    i8::from(jet_jit_expiring_get(handle, clock) != 0)
}

pub(crate) struct MemoryHostFns {
    pub allocator_new: cranelift_module::FuncId,
    pub allocator_alloc: cranelift_module::FuncId,
    pub allocator_reset: cranelift_module::FuncId,
    pub pool_new: cranelift_module::FuncId,
    pub pool_add: cranelift_module::FuncId,
    pub pool_get: cranelift_module::FuncId,
    pub pool_remove: cranelift_module::FuncId,
    pub pool_ids: cranelift_module::FuncId,
    pub shared_new: cranelift_module::FuncId,
    pub shared_begin: cranelift_module::FuncId,
    pub shared_end_read: cranelift_module::FuncId,
    pub shared_end_write: cranelift_module::FuncId,
    pub shared_downgrade: cranelift_module::FuncId,
    pub shared_strong_count: cranelift_module::FuncId,
    pub shared_weak_upgrade: cranelift_module::FuncId,
    pub condition_new: cranelift_module::FuncId,
    pub condition_notify_one: cranelift_module::FuncId,
    pub condition_notify_all: cranelift_module::FuncId,
    pub shared_guard_begin: cranelift_module::FuncId,
    pub shared_guard_value: cranelift_module::FuncId,
    pub shared_guard_set_value: cranelift_module::FuncId,
    pub shared_guard_end: cranelift_module::FuncId,
    pub shared_guard_wait_once: cranelift_module::FuncId,
    pub shared_txn_begin: cranelift_module::FuncId,
    pub shared_txn_get: cranelift_module::FuncId,
    pub shared_txn_set: cranelift_module::FuncId,
    pub shared_txn_commit: cranelift_module::FuncId,
    pub shared_txn_abort: cranelift_module::FuncId,
    pub expiring_new: cranelift_module::FuncId,
    pub expiring_get: cranelift_module::FuncId,
    pub expiring_is_valid: cranelift_module::FuncId,
}

pub(crate) fn register_memory_symbols(builder: &mut cranelift_jit::JITBuilder) {
    builder.symbol("jet_jit_allocator_new", jet_jit_allocator_new as *const u8);
    builder.symbol(
        "jet_jit_allocator_alloc",
        jet_jit_allocator_alloc as *const u8,
    );
    builder.symbol(
        "jet_jit_allocator_reset",
        jet_jit_allocator_reset as *const u8,
    );
    builder.symbol("jet_jit_pool_new", jet_jit_pool_new as *const u8);
    builder.symbol("jet_jit_pool_add", jet_jit_pool_add as *const u8);
    builder.symbol("jet_jit_pool_get", jet_jit_pool_get as *const u8);
    builder.symbol("jet_jit_pool_remove", jet_jit_pool_remove as *const u8);
    builder.symbol("jet_jit_pool_ids", jet_jit_pool_ids as *const u8);
    builder.symbol("jet_jit_shared_new", jet_jit_shared_new as *const u8);
    builder.symbol("jet_jit_shared_begin", jet_jit_shared_begin as *const u8);
    builder.symbol(
        "jet_jit_shared_end_read",
        jet_jit_shared_end_read as *const u8,
    );
    builder.symbol(
        "jet_jit_shared_end_write",
        jet_jit_shared_end_write as *const u8,
    );
    builder.symbol(
        "jet_jit_shared_downgrade",
        jet_jit_shared_downgrade as *const u8,
    );
    builder.symbol(
        "jet_jit_shared_strong_count",
        jet_jit_shared_strong_count as *const u8,
    );
    builder.symbol(
        "jet_jit_shared_weak_upgrade",
        jet_jit_shared_weak_upgrade as *const u8,
    );
    builder.symbol("jet_jit_condition_new", jet_jit_condition_new as *const u8);
    builder.symbol(
        "jet_jit_condition_notify_one",
        jet_jit_condition_notify_one as *const u8,
    );
    builder.symbol(
        "jet_jit_condition_notify_all",
        jet_jit_condition_notify_all as *const u8,
    );
    builder.symbol(
        "jet_jit_shared_guard_begin",
        jet_jit_shared_guard_begin as *const u8,
    );
    builder.symbol(
        "jet_jit_shared_guard_value",
        jet_jit_shared_guard_value as *const u8,
    );
    builder.symbol(
        "jet_jit_shared_guard_set_value",
        jet_jit_shared_guard_set_value as *const u8,
    );
    builder.symbol(
        "jet_jit_shared_guard_end",
        jet_jit_shared_guard_end as *const u8,
    );
    builder.symbol(
        "jet_jit_shared_guard_wait_once",
        jet_jit_shared_guard_wait_once as *const u8,
    );
    builder.symbol("jet_jit_shared_txn_begin", jet_jit_shared_txn_begin as *const u8);
    builder.symbol("jet_jit_shared_txn_get", jet_jit_shared_txn_get as *const u8);
    builder.symbol("jet_jit_shared_txn_set", jet_jit_shared_txn_set as *const u8);
    builder.symbol(
        "jet_jit_shared_txn_commit",
        jet_jit_shared_txn_commit as *const u8,
    );
    builder.symbol(
        "jet_jit_shared_txn_abort",
        jet_jit_shared_txn_abort as *const u8,
    );
    builder.symbol("jet_jit_expiring_new", jet_jit_expiring_new as *const u8);
    builder.symbol("jet_jit_expiring_get", jet_jit_expiring_get as *const u8);
    builder.symbol(
        "jet_jit_expiring_is_valid",
        jet_jit_expiring_is_valid as *const u8,
    );
}

pub(crate) fn declare_memory_host_fns(
    module: &mut cranelift_jit::JITModule,
) -> Result<MemoryHostFns, String> {
    use cranelift_codegen::ir::{types, AbiParam, Signature};
    use cranelift_module::{Linkage, Module};

    let cc = module.target_config().default_call_conv;
    let mut noarg_i64 = Signature::new(cc);
    noarg_i64.returns.push(AbiParam::new(types::I64));
    let mut unary = Signature::new(cc);
    unary.params.push(AbiParam::new(types::I64));
    unary.returns.push(AbiParam::new(types::I64));
    let mut binary = unary.clone();
    binary.params.push(AbiParam::new(types::I64));
    let mut unary_void = Signature::new(cc);
    unary_void.params.push(AbiParam::new(types::I64));
    let mut binary_void = unary_void.clone();
    binary_void.params.push(AbiParam::new(types::I64));
    let mut quaternary = Signature::new(cc);
    for _ in 0..4 {
        quaternary.params.push(AbiParam::new(types::I64));
    }
    quaternary.returns.push(AbiParam::new(types::I64));
    let mut binary_i8 = Signature::new(cc);
    binary_i8.params.push(AbiParam::new(types::I64));
    binary_i8.params.push(AbiParam::new(types::I64));
    binary_i8.returns.push(AbiParam::new(types::I8));
    let mut import = |name: &str, sig: &Signature| {
        module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|error| error.to_string())
    };

    Ok(MemoryHostFns {
        allocator_new: import("jet_jit_allocator_new", &noarg_i64)?,
        allocator_alloc: import("jet_jit_allocator_alloc", &binary)?,
        allocator_reset: import("jet_jit_allocator_reset", &unary_void)?,
        pool_new: import("jet_jit_pool_new", &noarg_i64)?,
        pool_add: import("jet_jit_pool_add", &binary)?,
        pool_get: import("jet_jit_pool_get", &binary)?,
        pool_remove: import("jet_jit_pool_remove", &binary)?,
        pool_ids: import("jet_jit_pool_ids", &unary)?,
        shared_new: import("jet_jit_shared_new", &unary)?,
        shared_begin: import("jet_jit_shared_begin", &unary)?,
        shared_end_read: import("jet_jit_shared_end_read", &unary_void)?,
        shared_end_write: import("jet_jit_shared_end_write", &binary_void)?,
        shared_downgrade: import("jet_jit_shared_downgrade", &unary)?,
        shared_strong_count: import("jet_jit_shared_strong_count", &unary)?,
        shared_weak_upgrade: import("jet_jit_shared_weak_upgrade", &unary)?,
        condition_new: import("jet_jit_condition_new", &noarg_i64)?,
        condition_notify_one: import("jet_jit_condition_notify_one", &unary_void)?,
        condition_notify_all: import("jet_jit_condition_notify_all", &unary_void)?,
        shared_guard_begin: import("jet_jit_shared_guard_begin", &binary)?,
        shared_guard_value: import("jet_jit_shared_guard_value", &unary)?,
        shared_guard_set_value: import("jet_jit_shared_guard_set_value", &binary_void)?,
        shared_guard_end: import("jet_jit_shared_guard_end", &unary_void)?,
        shared_guard_wait_once: import("jet_jit_shared_guard_wait_once", &binary_void)?,
        shared_txn_begin: import("jet_jit_shared_txn_begin", &Signature::new(cc))?,
        shared_txn_get: import("jet_jit_shared_txn_get", &unary)?,
        shared_txn_set: import("jet_jit_shared_txn_set", &binary_void)?,
        shared_txn_commit: import("jet_jit_shared_txn_commit", &Signature::new(cc))?,
        shared_txn_abort: import("jet_jit_shared_txn_abort", &Signature::new(cc))?,
        expiring_new: import("jet_jit_expiring_new", &quaternary)?,
        expiring_get: import("jet_jit_expiring_get", &binary)?,
        expiring_is_valid: import("jet_jit_expiring_is_valid", &binary_i8)?,
    })
}
