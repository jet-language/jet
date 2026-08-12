//! Native memory carriers for the resident Cranelift runtime.

use super::Concurrency;
use std::sync::atomic::{AtomicI64, Ordering, compiler_fence};
use std::sync::{Arc, Mutex};

pub(crate) mod shared_protocol {
    include!("../../jet-codegen/src/Prelude/SharedProtocol.rs");
}

thread_local! {
    static SHARED_TRANSACTIONS: std::cell::RefCell<Vec<SharedTransaction>> =
        const { std::cell::RefCell::new(Vec::new()) };
    static SHARED_ACTIVE_PERMITS:
        std::cell::RefCell<Vec<(i64, Arc<shared_protocol::JetSharedPermit>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

struct SharedTransaction {
    transaction: shared_protocol::JetSharedTransaction,
}

type SharedTransactionCallback = unsafe extern "C" fn(i64, i64) -> i64;

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
    pub(crate) protocol: Arc<shared_protocol::JetSharedProtocol>,
    value: AtomicI64,
}

pub(crate) struct ConditionState {
    pub(crate) protocol: Arc<shared_protocol::JetConditionProtocol>,
}

impl ConditionState {
    fn new() -> Self {
        Self {
            protocol: shared_protocol::JetConditionProtocol::new(),
        }
    }

    fn notify_one(&self) {
        shared_protocol::jet_shared_condition_notify_one(&self.protocol);
    }

    fn notify_all(&self) {
        shared_protocol::jet_shared_condition_notify_all(&self.protocol);
    }
}

struct JitConditionWaiter {
    slot: Arc<jet_codegen::scheduler::ParkSlot>,
}

impl JitConditionWaiter {
    fn new() -> Self {
        Self {
            slot: jet_codegen::scheduler::ParkSlot::new(),
        }
    }
}

impl shared_protocol::JetConditionWaiter for JitConditionWaiter {
    fn park(&self) -> Result<(), ()> {
        jet_codegen::scheduler::jet_scheduler_yield("Shared condition", &self.slot, None);
        Ok(())
    }

    fn wake(&self) {
        jet_codegen::scheduler::jet_scheduler_wake(&self.slot);
    }

    fn interrupted(&self) -> bool {
        jet_codegen::scheduler::jet_scheduler_wait_point_interrupted()
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
            protocol: shared_protocol::JetSharedProtocol::new(),
            value: AtomicI64::new(value),
        }
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

fn guard_shared_handle(rt: &crate::JitRuntime, guard: i64) -> Option<i64> {
    let handle = rt.heap.record_get_int(guard, GUARD_SHARED)?;
    (handle != 0).then_some(handle)
}

fn guard_state(
    rt: &crate::JitRuntime,
    guard: i64,
) -> Option<Arc<shared_protocol::JetSharedGuardState>> {
    rt.shared_guard_states.get(&guard).cloned()
}

fn guard_projection_slot(
    rt: &crate::JitRuntime,
    guard: i64,
    path: &[i64],
) -> Option<(i64, i64)> {
    if path.is_empty() {
        return Some((guard, GUARD_VALUE));
    }
    let mut record = rt.heap.record_get_int(guard, GUARD_VALUE)?;
    for field in path.iter().take(path.len().saturating_sub(1)) {
        record = rt.heap.record_get_int(record, *field)?;
    }
    Some((record, *path.last()?))
}

fn pack_shared_guard(
    rt: &mut crate::JitRuntime,
    shared_handle: i64,
    value: i64,
    state: Arc<shared_protocol::JetSharedGuardState>,
) -> i64 {
    let guard = rt.heap.alloc_record(2);
    let _ = rt.heap.record_set_int(guard, GUARD_SHARED, shared_handle);
    let _ = rt.heap.record_set_int(guard, GUARD_VALUE, value);
    rt.shared_guard_states.insert(guard, state);
    guard
}

fn take_active_shared_permit(handle: i64) -> Option<Arc<shared_protocol::JetSharedPermit>> {
    SHARED_ACTIVE_PERMITS.with(|permits| {
        let mut permits = permits.borrow_mut();
        permits
            .iter()
            .rposition(|(active_handle, _)| *active_handle == handle)
            .map(|index| permits.swap_remove(index).1)
    })
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

extern "C" fn jet_jit_shared_begin(handle: i64, editable: i64) -> i64 {
    let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, handle)) else {
        return 0;
    };
    let Some(permit) = shared_protocol::jet_shared_acquire(
        &shared.protocol,
        editable != 0,
        || false,
    ) else {
        return 0;
    };
    let value = shared.value.load(Ordering::Acquire);
    SHARED_ACTIVE_PERMITS.with(|permits| permits.borrow_mut().push((handle, permit)));
    value
}

extern "C" fn jet_jit_shared_end_read(handle: i64) {
    drop(take_active_shared_permit(handle));
}

extern "C" fn jet_jit_shared_end_write(handle: i64, value: i64) {
    if let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, handle)) {
        shared.value.store(value, Ordering::Release);
    }
    drop(take_active_shared_permit(handle));
}

/// D-SHARED-CYCLE1=C: weak handle is the same slot index; upgrade packs
/// Option as `0` (None) or `handle + 1` (Some).
extern "C" fn jet_jit_shared_downgrade(handle: i64) -> i64 {
    handle
}

extern "C" fn jet_jit_shared_strong_count(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        shared(rt, handle)
            // `shared()` clones the Arc out of `rt.shareds` to hand back an
            // owned handle, so `state` itself holds one strong ref the Jet
            // program never asked for; subtract it back out (D-SHARED-CYCLE1=C).
            .map(|state| (Arc::strong_count(&state) - 1) as i64)
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
        shared_protocol::jet_shared_condition_notify_one(&condition.protocol);
    }
}

extern "C" fn jet_jit_condition_notify_all(handle: i64) {
    if let Some(condition) = Concurrency::with_runtime_mut(|rt| condition(rt, handle)) {
        shared_protocol::jet_shared_condition_notify_all(&condition.protocol);
    }
}

extern "C" fn jet_jit_shared_guard_begin(handle: i64, editable: i64) -> i64 {
    let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, handle)) else {
        return Concurrency::with_runtime_mut(|rt| {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_INVALID);
            0
        });
    };
    let Some(state) = shared_protocol::jet_shared_guard_acquire(
        &shared.protocol,
        editable != 0,
        || false,
    ) else {
        return 0;
    };
    let value = shared.value.load(Ordering::Acquire);
    Concurrency::with_runtime_mut(|rt| {
        pack_shared_guard(
            rt,
            handle,
            value,
            state,
        )
    })
}

extern "C" fn jet_jit_shared_guard_map(guard: i64, field: i64, editable: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        if guard_shared_handle(rt, guard).is_none() {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_INVALID);
            return 0;
        }
        let Some(state) = guard_state(rt, guard) else {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_INVALID);
            return 0;
        };
        match shared_protocol::jet_shared_guard_map(
            &state,
            field,
            editable != 0,
        ) {
            Ok(mapped) => {
                let shared_handle = guard_shared_handle(rt, guard)
                    .expect("validated SharedGuard carrier lost its shared handle");
                let Some(value) = rt.heap.record_get_int(guard, GUARD_VALUE) else {
                    rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
                    return 0;
                };
                // Mapping consumes the source guard. Keep the source carrier
                // as an inert move marker and give the mapped projection its
                // own identity; the shared permit stays alive through the new
                // state, so lexical cleanup releases it exactly once.
                rt.shared_guard_states.remove(&guard);
                let _ = rt.heap.record_set_int(guard, GUARD_SHARED, 0);
                pack_shared_guard(rt, shared_handle, value, mapped)
            }
            Err(message) => {
                rt.set_trap(message);
                0
            }
        }
    })
}

extern "C" fn jet_jit_shared_guard_clone(guard: i64, editable: i64) -> i64 {
    let Some((shared, value, state)) = Concurrency::with_runtime_mut(|rt| {
        let shared = guard_shared_handle(rt, guard)?;
        let value = rt.heap.record_get_int(guard, GUARD_VALUE)?;
        let state = rt.shared_guard_states.get(&guard)?.clone();
        Some((shared, value, state))
    }) else {
        Concurrency::with_runtime_mut(|rt| {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_INVALID);
        });
        return 0;
    };
    let state = match shared_protocol::jet_shared_guard_clone(&state, editable != 0) {
        Ok(state) => state,
        Err(message) => {
            return Concurrency::with_runtime_mut(|rt| {
                rt.set_trap(message);
                0
            });
        }
    };
    Concurrency::with_runtime_mut(|rt| {
        pack_shared_guard(rt, shared, value, state)
    })
}

extern "C" fn jet_jit_shared_guard_value(guard: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some((record, field)) = readable_guard_slot(rt, guard) else {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_INVALID);
            return 0;
        };
        let Some(value) = rt.heap.record_get_int(record, field) else {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
            return 0;
        };
        value
    })
}

extern "C" fn jet_jit_shared_guard_value_f64(guard: i64) -> f64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some((record, field)) = readable_guard_slot(rt, guard) else {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_INVALID);
            return 0.0;
        };
        if field == GUARD_VALUE && record == guard {
            let Some(value) = rt.heap.record_get_int(record, field) else {
                rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
                return 0.0;
            };
            return f64::from_bits(value as u64);
        }
        let Some(value) = rt.heap.record_get_float(record, field) else {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
            return 0.0;
        };
        value
    })
}

extern "C" fn jet_jit_shared_guard_value_bool(guard: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        let Some((record, field)) = readable_guard_slot(rt, guard) else {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_INVALID);
            return 0;
        };
        if field == GUARD_VALUE && record == guard {
            let Some(value) = rt.heap.record_get_int(record, field) else {
                rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
                return 0;
            };
            return i8::from(value != 0);
        }
        let Some(value) = rt.heap.record_get_bool(record, field) else {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
            return 0;
        };
        i8::from(value)
    })
}

extern "C" fn jet_jit_shared_guard_value_char(guard: i64) -> i32 {
    Concurrency::with_runtime_mut(|rt| {
        let Some((record, field)) = readable_guard_slot(rt, guard) else {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_INVALID);
            return 0;
        };
        let value = if field == GUARD_VALUE && record == guard {
            let Some(value) = rt.heap.record_get_int(record, field) else {
                rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
                return 0;
            };
            value as i32
        } else {
            let Some(value) = rt.heap.record_get_char(record, field) else {
                rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
                return 0;
            };
            value as i32
        };
        if shared_protocol::jet_shared_guard_validate_char(value).is_err() {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_CHARACTER_STORAGE_FAILED);
            return 0;
        }
        value
    })
}

extern "C" fn jet_jit_shared_guard_value_string(guard: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some((record, field)) = readable_guard_slot(rt, guard) else {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_INVALID);
            return 0;
        };
        if field == GUARD_VALUE && record == guard {
            let Some(value) = rt.heap.record_get_int(record, field) else {
                rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
                return 0;
            };
            return value;
        }
        let Some(value) = rt.heap.record_get_string(record, field) else {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
            return 0;
        };
        value
    })
}

fn readable_guard_slot(
    rt: &crate::JitRuntime,
    guard: i64,
) -> Option<(i64, i64)> {
    guard_shared_handle(rt, guard)?;
    let state = guard_state(rt, guard)?;
    if !state.held() {
        return None;
    }
    guard_projection_slot(rt, guard, state.path())
}

fn editable_guard_slot(
    rt: &crate::JitRuntime,
    guard: i64,
) -> Result<(i64, i64, bool), &'static str> {
    guard_shared_handle(rt, guard).ok_or(shared_protocol::JET_SHARED_GUARD_INVALID)?;
    let state = guard_state(rt, guard).ok_or(shared_protocol::JET_SHARED_GUARD_INVALID)?;
    shared_protocol::jet_shared_guard_require_edit(&state)?;
    let (record, field) = guard_projection_slot(rt, guard, state.path())
        .ok_or(shared_protocol::JET_SHARED_GUARD_INVALID)?;
    Ok((record, field, state.path().is_empty()))
}

fn editable_guard_slot_or_trap(
    rt: &mut crate::JitRuntime,
    guard: i64,
) -> Option<(i64, i64, bool)> {
    match editable_guard_slot(rt, guard) {
        Ok(slot) => Some(slot),
        Err(message) => {
            rt.set_trap(message);
            None
        }
    }
}

fn store_root_guard_value(
    rt: &mut crate::JitRuntime,
    guard: i64,
    value: i64,
) -> Result<(), &'static str> {
    let shared_handle = guard_shared_handle(rt, guard)
        .ok_or(shared_protocol::JET_SHARED_GUARD_INVALID)?;
    let shared = shared(rt, shared_handle).ok_or(shared_protocol::JET_SHARED_GUARD_INVALID)?;
    shared.value.store(value, Ordering::Release);
    Ok(())
}

extern "C" fn jet_jit_shared_guard_set_value(guard: i64, value: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let Some((record, field, root)) = editable_guard_slot_or_trap(rt, guard) else {
            return;
        };
        if rt.heap.record_set_int(record, field, value).is_none() {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
            return;
        }
        if root {
            if store_root_guard_value(rt, guard, value).is_err() {
                rt.set_trap(shared_protocol::JET_SHARED_GUARD_INVALID);
            }
        }
    });
}

extern "C" fn jet_jit_shared_guard_set_value_f64(guard: i64, value: f64) {
    Concurrency::with_runtime_mut(|rt| {
        let Some((record, field, root)) = editable_guard_slot_or_trap(rt, guard) else {
            return;
        };
        if root {
            let bits = value.to_bits() as i64;
            if rt.heap.record_set_int(record, field, bits).is_none() {
                rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
                return;
            }
            if store_root_guard_value(rt, guard, bits).is_err() {
                rt.set_trap(shared_protocol::JET_SHARED_GUARD_INVALID);
            }
        } else if rt.heap.record_set_float(record, field, value).is_none() {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
        }
    });
}

extern "C" fn jet_jit_shared_guard_set_value_bool(guard: i64, value: i8) {
    Concurrency::with_runtime_mut(|rt| {
        let Some((record, field, root)) = editable_guard_slot_or_trap(rt, guard) else {
            return;
        };
        let value = value != 0;
        if root {
            let value = i64::from(value);
            if rt.heap.record_set_int(record, field, value).is_none() {
                rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
                return;
            }
            if store_root_guard_value(rt, guard, value).is_err() {
                rt.set_trap(shared_protocol::JET_SHARED_GUARD_INVALID);
            }
        } else if rt.heap.record_set_bool(record, field, value).is_none() {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
        }
    });
}

extern "C" fn jet_jit_shared_guard_set_value_char(guard: i64, value: i32) {
    Concurrency::with_runtime_mut(|rt| {
        let Some((record, field, root)) = editable_guard_slot_or_trap(rt, guard) else {
            return;
        };
        let value = match shared_protocol::jet_shared_guard_validate_char(value) {
            Ok(value) => value,
            Err(message) => {
                rt.set_trap(message);
                return;
            }
        };
        if root {
            let value = i64::from(value as u32);
            if rt.heap.record_set_int(record, field, value).is_none() {
                rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
                return;
            }
            if store_root_guard_value(rt, guard, value).is_err() {
                rt.set_trap(shared_protocol::JET_SHARED_GUARD_INVALID);
            }
        } else if rt.heap.record_set_char(record, field, value).is_none() {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
        }
    });
}

extern "C" fn jet_jit_shared_guard_set_value_string(guard: i64, value: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let Some((record, field, root)) = editable_guard_slot_or_trap(rt, guard) else {
            return;
        };
        if root {
            if rt.heap.record_set_int(record, field, value).is_none() {
                rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
                return;
            }
            if store_root_guard_value(rt, guard, value).is_err() {
                rt.set_trap(shared_protocol::JET_SHARED_GUARD_INVALID);
            }
        } else if rt.heap.record_set_string(record, field, value).is_none() {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
        }
    });
}

extern "C" fn jet_jit_shared_guard_end(guard: i64) {
    let Some((shared, value, editable, root, state)) = Concurrency::with_runtime_mut(|rt| {
        let Some(shared_handle) = guard_shared_handle(rt, guard) else {
            return None;
        };
        let state = rt.shared_guard_states.remove(&guard)?;
        let root = state.path().is_empty();
        let value = if root {
            let Some(value) = rt.heap.record_get_int(guard, GUARD_VALUE) else {
                rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
                return None;
            };
            value
        } else {
            0
        };
        let editable = state.editable();
        let _ = rt.heap.record_set_int(guard, GUARD_SHARED, 0);
        Some((
            shared(rt, shared_handle),
            value,
            editable,
            root,
            state,
        ))
    }) else {
        return;
    };
    if let Some(shared) = shared {
        if editable && root {
            shared.value.store(value, Ordering::Release);
        }
    }
    drop(state);
}

fn shared_guard_result(
    rt: &mut crate::JitRuntime,
    ok: bool,
    message: Option<&str>,
) -> i64 {
    let bits = message
        .map(|message| rt.heap.alloc_string(message.to_string()) as u64)
        .unwrap_or(0);
    crate::runtime_host::alloc_jit_result(rt, ok, bits)
}

extern "C" fn jet_jit_shared_guard_wait_once(guard: i64, condition_handle: i64) -> i64 {
    let Some((shared_handle, state, condition)) = Concurrency::with_runtime_mut(|rt| {
        let shared_handle = guard_shared_handle(rt, guard)?;
        let state = guard_state(rt, guard)?;
        let condition = condition(rt, condition_handle)?;
        Some((shared_handle, state, condition))
    }) else {
        return Concurrency::with_runtime_mut(|rt| {
            rt.set_trap(shared_protocol::JetSharedGuardWaitError::Invalid.message());
            shared_guard_result(
                rt,
                false,
                Some(shared_protocol::JetSharedGuardWaitError::Invalid.message()),
            )
        });
    };
    let waiter = Arc::new(JitConditionWaiter::new());
    let waited = jet_codegen::scheduler::jet_scheduler_wait_without_unwind(|| {
        shared_protocol::jet_shared_guard_wait_once(
            Some(state.as_ref()),
            Some(&condition.protocol),
            waiter,
        )
    });
    match waited {
        jet_codegen::scheduler::JetSchedulerWait::Ready(Ok(())) => {
            let Some(fresh) = Concurrency::with_runtime_mut(|rt| {
                Some(shared(rt, shared_handle)?.value.load(Ordering::Acquire))
            }) else {
                return Concurrency::with_runtime_mut(|rt| {
                    rt.set_trap(shared_protocol::JetSharedGuardWaitError::Invalid.message());
                    shared_guard_result(
                        rt,
                        false,
                        Some(shared_protocol::JetSharedGuardWaitError::Invalid.message()),
                    )
                });
            };
            Concurrency::with_runtime_mut(|rt| {
                if rt.heap.record_set_int(guard, GUARD_VALUE, fresh).is_none() {
                    rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
                    return shared_guard_result(
                        rt,
                        false,
                        Some(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED),
                    );
                }
                shared_guard_result(rt, true, None)
            })
        }
        jet_codegen::scheduler::JetSchedulerWait::Ready(Err(error)) => {
            Concurrency::with_runtime_mut(|rt| {
                if error.traps() {
                    rt.set_trap(error.message());
                }
                shared_guard_result(rt, false, Some(error.message()))
            })
        }
        jet_codegen::scheduler::JetSchedulerWait::Cancelled => Concurrency::with_runtime_mut(
            |rt| {
                shared_guard_result(
                    rt,
                    false,
                    Some(shared_protocol::JetSharedGuardWaitError::Cancelled.message()),
                )
            },
        ),
        jet_codegen::scheduler::JetSchedulerWait::Deadline(rendered) => {
            Concurrency::with_runtime_mut(|rt| {
                rt.set_deadline(rendered);
                shared_guard_result(
                    rt,
                    false,
                    Some(shared_protocol::JetSharedGuardWaitError::Cancelled.message()),
                )
            })
        }
        jet_codegen::scheduler::JetSchedulerWait::Panicked(message) => {
            Concurrency::with_runtime_mut(|rt| {
                rt.set_trap(&message);
                shared_guard_result(rt, false, Some(message.as_str()))
            })
        }
    }
}

extern "C" fn jet_jit_shared_txn_begin() {
    SHARED_TRANSACTIONS.with(|transactions| {
        transactions.borrow_mut().push(SharedTransaction {
            transaction: shared_protocol::jet_shared_transaction_begin(),
        });
    });
}

extern "C" fn jet_jit_shared_txn_record(
    handle: i64,
    callback_ptr: i64,
    environment: i64,
    record: i64,
) -> i64 {
    let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, handle)) else {
        return 0;
    };
    if callback_ptr == 0 {
        return 0;
    }
    // The callback address is produced by Cranelift `func_addr` for the fixed
    // `(environment, current) -> updated` ABI above.
    let callback: SharedTransactionCallback = unsafe { std::mem::transmute(callback_ptr as usize) };
    let protocol = Arc::clone(&shared.protocol);
    let delta = Box::new(move || {
        let current = shared.value.load(Ordering::Acquire);
        let updated = unsafe { callback(environment, current) };
        Concurrency::with_runtime_mut(|rt| {
            if record != 0 {
                if rt
                    .heap
                    .record_assign_from(current, updated)
                    .is_none()
                {
                    rt.set_trap(shared_protocol::JET_SHARED_TRANSACTION_VALUE_STORAGE_FAILED);
                }
            } else {
                shared.value.store(updated, Ordering::Release);
            }
        });
    });
    let recorded = SHARED_TRANSACTIONS.with(|transactions| {
        let mut transactions = transactions.borrow_mut();
        let Some(transaction) = transactions.last_mut() else {
            return false;
        };
        transaction.transaction.record_edit(protocol, delta);
        true
    });
    i64::from(recorded)
}

extern "C" fn jet_jit_shared_txn_commit() {
    let Some(transaction) =
        SHARED_TRANSACTIONS.with(|transactions| transactions.borrow_mut().pop())
    else {
        return;
    };
    transaction.transaction.commit();
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

host_fns! {
    struct MemoryHostFns;
    register: register_memory_symbols;
    declare: declare_memory_host_fns(module) {
        use cranelift_codegen::ir::{types, AbiParam, Signature};
        use cranelift_module::{Linkage, Module};
        let cc = module.target_config().default_call_conv;
        let mut noarg_i64 = Signature::new(cc);
        noarg_i64.returns.push(AbiParam::new(types::I64));
        let mut unary = Signature::new(cc);
        unary.params.push(AbiParam::new(types::I64));
        unary.returns.push(AbiParam::new(types::I64));
        let mut unary_f64 = Signature::new(cc);
        unary_f64.params.push(AbiParam::new(types::I64));
        unary_f64.returns.push(AbiParam::new(types::F64));
        let mut unary_i8 = Signature::new(cc);
        unary_i8.params.push(AbiParam::new(types::I64));
        unary_i8.returns.push(AbiParam::new(types::I8));
        let mut unary_i32 = Signature::new(cc);
        unary_i32.params.push(AbiParam::new(types::I64));
        unary_i32.returns.push(AbiParam::new(types::I32));
        let mut binary = unary.clone();
        binary.params.push(AbiParam::new(types::I64));
        let mut ternary = binary.clone();
        ternary.params.push(AbiParam::new(types::I64));
        let mut unary_void = Signature::new(cc);
        unary_void.params.push(AbiParam::new(types::I64));
        let mut binary_void = unary_void.clone();
        binary_void.params.push(AbiParam::new(types::I64));
        let mut binary_f64_void = Signature::new(cc);
        binary_f64_void.params.push(AbiParam::new(types::I64));
        binary_f64_void.params.push(AbiParam::new(types::F64));
        let mut binary_i8_void = Signature::new(cc);
        binary_i8_void.params.push(AbiParam::new(types::I64));
        binary_i8_void.params.push(AbiParam::new(types::I8));
        let mut binary_i32_void = Signature::new(cc);
        binary_i32_void.params.push(AbiParam::new(types::I64));
        binary_i32_void.params.push(AbiParam::new(types::I32));
        let mut quaternary = Signature::new(cc);
        for _ in 0..4 {
            quaternary.params.push(AbiParam::new(types::I64));
        }
        quaternary.returns.push(AbiParam::new(types::I64));
        let mut binary_i8 = Signature::new(cc);
        binary_i8.params.push(AbiParam::new(types::I64));
        binary_i8.params.push(AbiParam::new(types::I64));
        binary_i8.returns.push(AbiParam::new(types::I8));


    }
    allocator_new: "jet_jit_allocator_new" => jet_jit_allocator_new: noarg_i64;
    allocator_alloc: "jet_jit_allocator_alloc" => jet_jit_allocator_alloc: binary;
    allocator_reset: "jet_jit_allocator_reset" => jet_jit_allocator_reset: unary_void;
    pool_new: "jet_jit_pool_new" => jet_jit_pool_new: noarg_i64;
    pool_add: "jet_jit_pool_add" => jet_jit_pool_add: binary;
    pool_get: "jet_jit_pool_get" => jet_jit_pool_get: binary;
    pool_remove: "jet_jit_pool_remove" => jet_jit_pool_remove: binary;
    pool_ids: "jet_jit_pool_ids" => jet_jit_pool_ids: unary;
    shared_new: "jet_jit_shared_new" => jet_jit_shared_new: unary;
    shared_begin: "jet_jit_shared_begin" => jet_jit_shared_begin: binary;
    shared_end_read: "jet_jit_shared_end_read" => jet_jit_shared_end_read: unary_void;
    shared_end_write: "jet_jit_shared_end_write" => jet_jit_shared_end_write: binary_void;
    shared_downgrade: "jet_jit_shared_downgrade" => jet_jit_shared_downgrade: unary;
    shared_strong_count: "jet_jit_shared_strong_count" => jet_jit_shared_strong_count: unary;
    shared_weak_upgrade: "jet_jit_shared_weak_upgrade" => jet_jit_shared_weak_upgrade: unary;
    condition_new: "jet_jit_condition_new" => jet_jit_condition_new: noarg_i64;
    condition_notify_one: "jet_jit_condition_notify_one" => jet_jit_condition_notify_one: unary_void;
    condition_notify_all: "jet_jit_condition_notify_all" => jet_jit_condition_notify_all: unary_void;
    shared_guard_begin: "jet_jit_shared_guard_begin" => jet_jit_shared_guard_begin: binary;
    shared_guard_map: "jet_jit_shared_guard_map" => jet_jit_shared_guard_map: ternary;
    shared_guard_clone: "jet_jit_shared_guard_clone" => jet_jit_shared_guard_clone: binary;
    shared_guard_value: "jet_jit_shared_guard_value" => jet_jit_shared_guard_value: unary;
    shared_guard_value_f64: "jet_jit_shared_guard_value_f64" => jet_jit_shared_guard_value_f64: unary_f64;
    shared_guard_value_bool: "jet_jit_shared_guard_value_bool" => jet_jit_shared_guard_value_bool: unary_i8;
    shared_guard_value_char: "jet_jit_shared_guard_value_char" => jet_jit_shared_guard_value_char: unary_i32;
    shared_guard_value_string: "jet_jit_shared_guard_value_string" => jet_jit_shared_guard_value_string: unary;
    shared_guard_set_value: "jet_jit_shared_guard_set_value" => jet_jit_shared_guard_set_value: binary_void;
    shared_guard_set_value_f64: "jet_jit_shared_guard_set_value_f64" => jet_jit_shared_guard_set_value_f64: binary_f64_void;
    shared_guard_set_value_bool: "jet_jit_shared_guard_set_value_bool" => jet_jit_shared_guard_set_value_bool: binary_i8_void;
    shared_guard_set_value_char: "jet_jit_shared_guard_set_value_char" => jet_jit_shared_guard_set_value_char: binary_i32_void;
    shared_guard_set_value_string: "jet_jit_shared_guard_set_value_string" => jet_jit_shared_guard_set_value_string: binary_void;
    shared_guard_end: "jet_jit_shared_guard_end" => jet_jit_shared_guard_end: unary_void;
    shared_guard_wait_once: "jet_jit_shared_guard_wait_once" => jet_jit_shared_guard_wait_once: binary;
    shared_txn_begin: "jet_jit_shared_txn_begin" => jet_jit_shared_txn_begin: Signature::new(cc);
    shared_txn_record: "jet_jit_shared_txn_record" => jet_jit_shared_txn_record: quaternary;
    shared_txn_commit: "jet_jit_shared_txn_commit" => jet_jit_shared_txn_commit: Signature::new(cc);
    shared_txn_abort: "jet_jit_shared_txn_abort" => jet_jit_shared_txn_abort: Signature::new(cc);
    expiring_new: "jet_jit_expiring_new" => jet_jit_expiring_new: quaternary;
    expiring_get: "jet_jit_expiring_get" => jet_jit_expiring_get: binary;
    expiring_is_valid: "jet_jit_expiring_is_valid" => jet_jit_expiring_is_valid: binary_i8;
}
