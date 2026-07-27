//! Native memory carriers for the resident Cranelift runtime.

use super::Concurrency;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

thread_local! {
    static SHARED_TRANSACTION: std::cell::RefCell<Vec<(i64, Arc<SharedState>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
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

pub(crate) struct ExpiringState {
    value: i64,
    expires_at: i64,
    clock: i64,
    secret: bool,
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
    value + 1
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

extern "C" fn jet_jit_shared_txn_begin() {
    SHARED_TRANSACTION.with(|transaction| transaction.borrow_mut().clear());
}

extern "C" fn jet_jit_shared_txn_touch(handle: i64) {
    let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, handle)) else {
        return;
    };
    SHARED_TRANSACTION.with(|transaction| {
        let mut transaction = transaction.borrow_mut();
        if !transaction.iter().any(|(existing, _)| *existing == handle) {
            transaction.push((handle, shared));
        }
    });
}

extern "C" fn jet_jit_shared_txn_lock() {
    SHARED_TRANSACTION.with(|transaction| {
        let mut transaction = transaction.borrow_mut();
        transaction.sort_by_key(|(handle, _)| *handle);
        for (_, shared) in transaction.iter() {
            shared.lock();
        }
    });
}

fn transaction_shared(handle: i64) -> Option<Arc<SharedState>> {
    SHARED_TRANSACTION.with(|transaction| {
        transaction
            .borrow()
            .iter()
            .find(|(existing, _)| *existing == handle)
            .map(|(_, shared)| Arc::clone(shared))
    })
}

extern "C" fn jet_jit_shared_txn_get(handle: i64) -> i64 {
    transaction_shared(handle)
        .map(|shared| shared.value.load(Ordering::Relaxed))
        .unwrap_or(0)
}

extern "C" fn jet_jit_shared_txn_set(handle: i64, value: i64) {
    if let Some(shared) = transaction_shared(handle) {
        shared.value.store(value, Ordering::Relaxed);
    }
}

extern "C" fn jet_jit_shared_txn_end() {
    SHARED_TRANSACTION.with(|transaction| {
        let mut transaction = transaction.borrow_mut();
        for (_, shared) in transaction.iter().rev() {
            shared.unlock();
        }
        transaction.clear();
    });
}

extern "C" fn jet_jit_expiring_new(
    value: i64,
    duration: i64,
    clock: i64,
    secret: i64,
) -> i64 {
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
            secret: secret != 0,
        });
        rt.expirings.len() as i64
    })
}

extern "C" fn jet_jit_expiring_get(handle: i64, clock: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
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
            return 0;
        };
        if now >= value.expires_at {
            if value.secret {
                value.value = 0;
            }
            return 0;
        }
        value.value + 1
    })
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
    pub shared_txn_begin: cranelift_module::FuncId,
    pub shared_txn_touch: cranelift_module::FuncId,
    pub shared_txn_lock: cranelift_module::FuncId,
    pub shared_txn_get: cranelift_module::FuncId,
    pub shared_txn_set: cranelift_module::FuncId,
    pub shared_txn_end: cranelift_module::FuncId,
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
    builder.symbol("jet_jit_shared_txn_begin", jet_jit_shared_txn_begin as *const u8);
    builder.symbol("jet_jit_shared_txn_touch", jet_jit_shared_txn_touch as *const u8);
    builder.symbol("jet_jit_shared_txn_lock", jet_jit_shared_txn_lock as *const u8);
    builder.symbol("jet_jit_shared_txn_get", jet_jit_shared_txn_get as *const u8);
    builder.symbol("jet_jit_shared_txn_set", jet_jit_shared_txn_set as *const u8);
    builder.symbol("jet_jit_shared_txn_end", jet_jit_shared_txn_end as *const u8);
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
        shared_txn_begin: import("jet_jit_shared_txn_begin", &Signature::new(cc))?,
        shared_txn_touch: import("jet_jit_shared_txn_touch", &unary_void)?,
        shared_txn_lock: import("jet_jit_shared_txn_lock", &Signature::new(cc))?,
        shared_txn_get: import("jet_jit_shared_txn_get", &unary)?,
        shared_txn_set: import("jet_jit_shared_txn_set", &binary_void)?,
        shared_txn_end: import("jet_jit_shared_txn_end", &Signature::new(cc))?,
        expiring_new: import("jet_jit_expiring_new", &quaternary)?,
        expiring_get: import("jet_jit_expiring_get", &binary)?,
        expiring_is_valid: import("jet_jit_expiring_is_valid", &binary_i8)?,
    })
}
