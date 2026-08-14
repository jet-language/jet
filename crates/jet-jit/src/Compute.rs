//! Resident adapters for the shared compute Prelude.
//!
//! The JIT owns only handles and list marshalling. Tensor construction,
//! copying, view bounds, and writes stay in the same Prelude source used by
//! AOT and the interpreter (I9).

use crate::JitRuntime;
use crate::Marshal::{result_err_msg, result_ok};
use super::Concurrency;

#[allow(dead_code, unused_imports)]
mod semantics {
    use crate::JetShow;
    use jet_foundation::Outcome::jet_list_bounds_message;
    use jet_foundation::StructuralDebug::jet_debug_range;

    fn jet_panic(_file: &str, _line: u32, message: &str) -> ! {
        panic!("{message}");
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct JetRange {
        start: i64,
        end: i64,
        exclusive: bool,
    }

    include!("../../jet-codegen/src/Prelude/Core/RangeBounds.rs");
    include!("../../jet-codegen/src/Prelude/Core/ViewAccess.rs");
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/Compute.rs");

    pub(super) type Tensor = JetTensor;

    pub(super) fn from_list(values: &[f64]) -> Result<Tensor, String> {
        jet_compute_from_list(&values.to_vec()).map_err(|error| error.jet_show())
    }

    pub(super) fn matrix(rows: i64, cols: i64, fill: f64) -> Result<Tensor, String> {
        jet_compute_matrix(rows, cols, fill).map_err(|error| error.jet_show())
    }

    pub(super) fn zeros(shape: &[i64]) -> Result<Tensor, String> {
        jet_compute_zeros(&shape.to_vec()).map_err(|error| error.jet_show())
    }

    pub(super) fn ones(shape: &[i64]) -> Result<Tensor, String> {
        jet_compute_ones(&shape.to_vec()).map_err(|error| error.jet_show())
    }

    pub(super) fn full(shape: &[i64], value: f64) -> Result<Tensor, String> {
        jet_compute_full(&shape.to_vec(), value).map_err(|error| error.jet_show())
    }

    pub(super) fn eye(size: i64) -> Result<Tensor, String> {
        jet_compute_eye(size).map_err(|error| error.jet_show())
    }

    pub(super) fn add(left: &Tensor, right: &Tensor) -> Result<Tensor, String> {
        jet_compute_add(left, right).map_err(|error| error.jet_show())
    }

    pub(super) fn mul(left: &Tensor, right: &Tensor) -> Result<Tensor, String> {
        jet_compute_mul(left, right).map_err(|error| error.jet_show())
    }

    pub(super) fn sub(left: &Tensor, right: &Tensor) -> Result<Tensor, String> {
        jet_compute_binary("sub", left, right).map_err(|error| error.jet_show())
    }

    pub(super) fn matmul(left: &Tensor, right: &Tensor) -> Result<Tensor, String> {
        jet_compute_matmul(left, right).map_err(|error| error.jet_show())
    }

    pub(super) fn sum_axis(tensor: &Tensor, axis: i64) -> Result<Tensor, String> {
        jet_compute_sum_axis(tensor, axis).map_err(|error| error.jet_show())
    }

    pub(super) fn mse_loss(left: &Tensor, right: &Tensor) -> Result<Tensor, String> {
        jet_compute_mse_loss(left, right).map_err(|error| error.jet_show())
    }

    pub(super) fn sgd_step(param: &Tensor, grad: &Tensor, learning_rate: f64) -> Result<Tensor, String> {
        jet_compute_sgd_step(param, grad, learning_rate).map_err(|error| error.jet_show())
    }

    pub(super) fn serialize(tensor: &Tensor) -> Result<String, String> {
        jet_compute_serialize(tensor).map_err(|error| error.jet_show())
    }

    pub(super) fn deserialize(payload: &str) -> Result<Tensor, String> {
        jet_compute_deserialize(&payload.to_string()).map_err(|error| error.jet_show())
    }

    pub(super) fn matmul_f32_tile(left: &Tensor, right: &Tensor) -> Result<Tensor, String> {
        jet_compute_matmul_f32_tile(left, right).map_err(|error| error.jet_show())
    }

    pub(super) fn set(tensor: &mut Tensor, indices: &[i64], value: f64) -> Result<(), String> {
        jet_compute_set(tensor, indices, value).map_err(|error| error.jet_show())
    }

    pub(super) fn get(tensor: &Tensor, indices: &[i64]) -> Result<f64, String> {
        jet_compute_get(tensor, indices).map_err(|error| error.jet_show())
    }

    pub(super) fn tensor_shape(tensor: &Tensor) -> Vec<i64> {
        jet_compute_tensor_shape(tensor)
    }

    pub(super) fn tensor_rank(tensor: &Tensor) -> i64 {
        jet_compute_tensor_rank(tensor)
    }

    pub(super) fn tensor_numel(tensor: &Tensor) -> i64 {
        jet_compute_tensor_numel(tensor)
    }

    pub(super) fn tensor_device(tensor: &Tensor) -> String {
        jet_compute_tensor_device(tensor)
    }

    pub(super) fn tensor_placement(tensor: &Tensor) -> String {
        jet_compute_tensor_placement(tensor)
    }

    pub(super) fn profile_show() -> String {
        jet_compute_profile_show()
    }

    pub(super) fn copy(tensor: &Tensor) -> Tensor {
        jet_compute_copy(tensor)
    }

    pub(super) fn clone(tensor: &Tensor) -> Tensor {
        tensor.clone()
    }

    pub(super) fn tensor_values(tensor: &Tensor) -> Result<Vec<f64>, String> {
        jet_compute_validate_tensor(tensor).map_err(|error| error.jet_show())?;
        Ok(jet_compute_tensor_to_list(tensor))
    }

    pub(super) fn slice(
        tensor: &Tensor,
        start: i64,
        end: i64,
        exclusive: bool,
    ) -> Result<Tensor, String> {
        jet_compute_slice_checked(tensor, start, end, exclusive)
            .map_err(|error| error.jet_show())
    }

    pub(super) fn view_values(
        tensor: &Tensor,
        start: i64,
        end: i64,
        exclusive: bool,
    ) -> Result<Vec<f64>, String> {
        jet_compute_view_checked(tensor, start, end, exclusive)
            .map(|view| view.to_vec())
            .map_err(|error| error.jet_show())
    }

    pub(super) fn window_get(
        tensor: &Tensor,
        start: i64,
        end: i64,
        exclusive: bool,
        index: i64,
    ) -> Result<f64, String> {
        jet_compute_window_get(tensor, start, end, exclusive, index)
    }

    pub(super) fn validate_mut(
        tensor: &mut Tensor,
        start: i64,
        end: i64,
        exclusive: bool,
    ) -> Result<(), String> {
        jet_compute_view_mut_checked(tensor, start, end, exclusive)
            .map(|_| ())
            .map_err(|error| error.jet_show())
    }

    /// Marshal one mutable Tensor-window write into the shared Prelude seam.
    pub(super) fn window_set(
        tensor: &mut Tensor,
        start: i64,
        end: i64,
        exclusive: bool,
        index: i64,
        value: f64,
    ) -> Result<(), String> {
        jet_compute_window_set(tensor, start, end, exclusive, index, value)
    }
}

#[derive(Default)]
pub(crate) struct ComputeState {
    slots: Vec<Option<TensorSlot>>,
    windows: Vec<TensorWindowSlot>,
}

impl ComputeState {
    pub(crate) fn clear(&mut self) {
        self.windows.clear();
        self.slots.clear();
    }
}

struct TensorSlot {
    tensor: semantics::Tensor,
    /// The flattened list handle used by the ordinary ViewMut JIT record.
    list: i64,
}

#[derive(Clone, Copy)]
struct TensorWindowSlot {
    /// The materialized list used by generic view operations.
    list: i64,
    /// The owning Tensor handle and its original Prelude window facts.
    tensor: i64,
    start: i64,
    end: i64,
    exclusive: bool,
}

fn tensor_index(handle: i64) -> Option<usize> {
    usize::try_from(handle).ok()?.checked_sub(1)
}

fn slot<'a>(runtime: &'a JitRuntime, handle: i64) -> Option<&'a TensorSlot> {
    tensor_index(handle)
        .and_then(|index| runtime.compute.slots.get(index))
        .and_then(Option::as_ref)
}

fn slot_mut<'a>(runtime: &'a mut JitRuntime, handle: i64) -> Option<&'a mut TensorSlot> {
    tensor_index(handle)
        .and_then(|index| runtime.compute.slots.get_mut(index))
        .and_then(Option::as_mut)
}

fn trap(runtime: &mut JitRuntime, message: &str) -> i64 {
    runtime.set_trap(message);
    0
}

fn read_float_list(runtime: &JitRuntime, handle: i64) -> Option<Vec<f64>> {
    let len = runtime.heap.list_len(handle)?;
    (0..len)
        .map(|index| runtime.heap.list_get_float(handle, index))
        .collect()
}

fn alloc_float_list(runtime: &mut JitRuntime, values: &[f64]) -> i64 {
    let list = runtime.heap.alloc_empty_list();
    for &value in values {
        if runtime.heap.list_push_float(list, value).is_none() {
            runtime.set_trap("JIT compute list allocation failed");
            return 0;
        }
    }
    list
}

fn alloc_int_list(runtime: &mut JitRuntime, values: &[i64]) -> i64 {
    let list = runtime.heap.alloc_empty_list();
    for &value in values {
        if runtime.heap.list_push_int(list, value).is_none() {
            runtime.set_trap("JIT compute integer-list allocation failed");
            return 0;
        }
    }
    list
}

fn read_int_list(runtime: &JitRuntime, handle: i64) -> Option<Vec<i64>> {
    let len = runtime.heap.list_len(handle)?;
    (0..len)
        .map(|index| runtime.heap.list_get_int(handle, index))
        .collect()
}

fn sync_float_list(runtime: &mut JitRuntime, list: i64, values: &[f64]) -> bool {
    for (index, &value) in values.iter().enumerate() {
        let Ok(index) = i64::try_from(index) else {
            runtime.set_trap("JIT compute list index overflow");
            return false;
        };
        if runtime.heap.list_set_float(list, index, value).is_none() {
            runtime.set_trap("JIT compute list mirror is invalid");
            return false;
        }
    }
    true
}

fn alloc_tensor(runtime: &mut JitRuntime, tensor: semantics::Tensor) -> i64 {
    let Ok(values) = semantics::tensor_values(&tensor) else {
        runtime.set_trap("JIT compute tensor validation failed");
        return 0;
    };
    let list = alloc_float_list(runtime, &values);
    runtime.compute.slots.push(Some(TensorSlot { tensor, list }));
    runtime.compute.slots.len() as i64
}

extern "C" fn jet_jit_compute_drop_tensor(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(index) = tensor_index(tensor) else {
            return 0;
        };
        let released = runtime
            .compute
            .slots
            .get_mut(index)
            .and_then(Option::take)
            .is_some();
        if released {
            runtime
                .compute
                .windows
                .retain(|window| window.tensor != tensor);
        }
        0
    })
}

extern "C" fn jet_jit_compute_drop_window(list: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        runtime.compute.windows.retain(|window| window.list != list);
        0
    })
}

extern "C" fn jet_jit_compute_from_list(values: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(values) = read_float_list(runtime, values) else {
            return trap(runtime, "core.compute.from_list expects a float list");
        };
        match semantics::from_list(&values) {
            Ok(tensor) => {
                let handle = alloc_tensor(runtime, tensor);
                result_ok(handle as u64)
            }
            Err(message) => trap(runtime, &message),
        }
    })
}

extern "C" fn jet_jit_compute_matrix(rows: i64, cols: i64, fill: f64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| match semantics::matrix(rows, cols, fill) {
        Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
        Err(message) => result_err_msg(&message),
    })
}

extern "C" fn jet_jit_compute_zeros(shape: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(shape) = read_int_list(runtime, shape) else {
            return result_err_msg("core.compute.zeros expects an integer shape list");
        };
        match semantics::zeros(&shape) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

extern "C" fn jet_jit_compute_ones(shape: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(shape) = read_int_list(runtime, shape) else {
            return result_err_msg("core.compute.ones expects an integer shape list");
        };
        match semantics::ones(&shape) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

extern "C" fn jet_jit_compute_full(shape: i64, value: f64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(shape) = read_int_list(runtime, shape) else {
            return result_err_msg("core.compute.full expects an integer shape list");
        };
        match semantics::full(&shape, value) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

extern "C" fn jet_jit_compute_eye(size: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| match semantics::eye(size) {
        Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
        Err(message) => result_err_msg(&message),
    })
}

fn clone_tensor_pair(runtime: &JitRuntime, left: i64, right: i64) -> Option<(semantics::Tensor, semantics::Tensor)> {
    Some((slot(runtime, left)?.tensor.clone(), slot(runtime, right)?.tensor.clone()))
}

extern "C" fn jet_jit_compute_add(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some((left, right)) = clone_tensor_pair(runtime, left, right) else {
            return result_err_msg("core.compute.add received an invalid Tensor handle");
        };
        match semantics::add(&left, &right) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

extern "C" fn jet_jit_compute_mul(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some((left, right)) = clone_tensor_pair(runtime, left, right) else {
            return result_err_msg("core.compute.mul received an invalid Tensor handle");
        };
        match semantics::mul(&left, &right) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

extern "C" fn jet_jit_compute_sub(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some((left, right)) = clone_tensor_pair(runtime, left, right) else {
            return result_err_msg("core.compute.sub received an invalid Tensor handle");
        };
        match semantics::sub(&left, &right) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

extern "C" fn jet_jit_compute_matmul(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some((left, right)) = clone_tensor_pair(runtime, left, right) else {
            return result_err_msg("core.compute.matmul received an invalid Tensor handle");
        };
        match semantics::matmul(&left, &right) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

extern "C" fn jet_jit_compute_sum_axis(tensor: i64, axis: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return result_err_msg("core.compute.sum_axis received an invalid Tensor handle");
        };
        match semantics::sum_axis(&tensor, axis) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

extern "C" fn jet_jit_compute_mse_loss(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some((left, right)) = clone_tensor_pair(runtime, left, right) else {
            return result_err_msg("core.compute.mse_loss received an invalid Tensor handle");
        };
        match semantics::mse_loss(&left, &right) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

extern "C" fn jet_jit_compute_sgd_step(param: i64, grad: i64, learning_rate: f64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some((param, grad)) = clone_tensor_pair(runtime, param, grad) else {
            return result_err_msg("core.compute.sgd_step received an invalid Tensor handle");
        };
        match semantics::sgd_step(&param, &grad, learning_rate) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

extern "C" fn jet_jit_compute_serialize(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return result_err_msg("core.compute.serialize received an invalid Tensor handle");
        };
        match semantics::serialize(&tensor) {
            Ok(payload) => {
                let handle = runtime.heap.alloc_string(payload);
                result_ok(handle as u64)
            }
            Err(message) => result_err_msg(&message),
        }
    })
}

extern "C" fn jet_jit_compute_deserialize(payload: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(payload) = runtime.heap.clone_string(payload) else {
            return result_err_msg("core.compute.deserialize expects a string payload");
        };
        match semantics::deserialize(&payload) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

extern "C" fn jet_jit_compute_matmul_f32_tile(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some((left, right)) = clone_tensor_pair(runtime, left, right) else {
            return result_err_msg(
                "core.compute.matmul_f32_tile received an invalid Tensor handle",
            );
        };
        match semantics::matmul_f32_tile(&left, &right) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

extern "C" fn jet_jit_compute_get(tensor: i64, indices: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(indices) = read_int_list(runtime, indices) else {
            return result_err_msg("core.compute.get expects an integer index list");
        };
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return result_err_msg("core.compute.get received an invalid Tensor handle");
        };
        match semantics::get(&tensor, &indices) {
            Ok(value) => result_ok(value.to_bits()),
            Err(message) => result_err_msg(&message),
        }
    })
}

extern "C" fn jet_jit_compute_set(tensor: i64, indices: i64, value: f64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(indices) = read_int_list(runtime, indices) else {
            return result_err_msg("core.compute.set expects an integer index list");
        };
        let result = {
            let Some(slot) = slot_mut(runtime, tensor) else {
                return result_err_msg("core.compute.set received an invalid Tensor handle");
            };
            semantics::set(&mut slot.tensor, &indices, value)
        };
        match result {
            Ok(()) => {
                let values = match slot(runtime, tensor) {
                    Some(slot) => match semantics::tensor_values(&slot.tensor) {
                        Ok(values) => values,
                        Err(message) => return result_err_msg(&message),
                    },
                    None => return result_err_msg("core.compute.set received an invalid Tensor handle"),
                };
                let Some(list) = slot(runtime, tensor).map(|slot| slot.list) else {
                    return result_err_msg("core.compute.set received an invalid Tensor handle");
                };
                if !sync_float_list(runtime, list, &values) {
                    return 0;
                }
                result_ok(0)
            }
            Err(message) => result_err_msg(&message),
        }
    })
}

extern "C" fn jet_jit_compute_shape(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return trap(runtime, "core.compute.shape received an invalid Tensor handle");
        };
        alloc_int_list(runtime, &semantics::tensor_shape(&tensor))
    })
}

extern "C" fn jet_jit_compute_rank(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| match slot(runtime, tensor) {
        Some(slot) => semantics::tensor_rank(&slot.tensor),
        None => trap(runtime, "core.compute.rank received an invalid Tensor handle"),
    })
}

extern "C" fn jet_jit_compute_numel(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| match slot(runtime, tensor) {
        Some(slot) => semantics::tensor_numel(&slot.tensor),
        None => trap(runtime, "core.compute.numel received an invalid Tensor handle"),
    })
}

extern "C" fn jet_jit_compute_device(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return trap(runtime, "core.compute.device received an invalid Tensor handle");
        };
        runtime.heap.alloc_string(semantics::tensor_device(&tensor))
    })
}

extern "C" fn jet_jit_compute_placement(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return trap(runtime, "core.compute.placement received an invalid Tensor handle");
        };
        runtime.heap.alloc_string(semantics::tensor_placement(&tensor))
    })
}

extern "C" fn jet_jit_compute_profile_show() -> i64 {
    Concurrency::with_runtime_mut(|runtime| runtime.heap.alloc_string(semantics::profile_show()))
}

extern "C" fn jet_jit_compute_copy(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let copied = match slot(runtime, tensor) {
            Some(slot) => semantics::copy(&slot.tensor),
            None => return trap(runtime, "core.compute.copy received an invalid Tensor handle"),
        };
        alloc_tensor(runtime, copied)
    })
}

extern "C" fn jet_jit_compute_clone(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let cloned = match slot(runtime, tensor) {
            Some(slot) => semantics::clone(&slot.tensor),
            None => return trap(runtime, "core.compute.clone received an invalid Tensor handle"),
        };
        alloc_tensor(runtime, cloned)
    })
}

extern "C" fn jet_jit_compute_tensor_to_list(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(slot) = slot(runtime, tensor) else {
            return trap(runtime, "core.compute.to_list received an invalid Tensor handle");
        };
        let values = match semantics::tensor_values(&slot.tensor) {
            Ok(values) => values,
            Err(message) => return trap(runtime, &message),
        };
        alloc_float_list(runtime, &values)
    })
}

extern "C" fn jet_jit_compute_slice(
    tensor: i64,
    start: i64,
    end: i64,
    exclusive: i8,
) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(slot) = slot(runtime, tensor) else {
            return trap(runtime, "core.compute slice received an invalid Tensor handle");
        };
        let sliced = match semantics::slice(&slot.tensor, start, end, exclusive != 0) {
            Ok(tensor) => tensor,
            Err(message) => return trap(runtime, &message),
        };
        alloc_tensor(runtime, sliced)
    })
}

extern "C" fn jet_jit_compute_view(
    tensor: i64,
    start: i64,
    end: i64,
    exclusive: i8,
) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(slot) = slot(runtime, tensor) else {
            return trap(runtime, "core.compute view received an invalid Tensor handle");
        };
        let values = match semantics::view_values(&slot.tensor, start, end, exclusive != 0) {
            Ok(values) => values,
            Err(message) => return trap(runtime, &message),
        };
        alloc_float_list(runtime, &values)
    })
}

extern "C" fn jet_jit_compute_view_mut(
    tensor: i64,
    start: i64,
    end: i64,
    exclusive: i8,
) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let values = {
            let Some(slot) = slot_mut(runtime, tensor) else {
                return trap(runtime, "core.compute mutable view received an invalid Tensor handle");
            };
            if let Err(message) = semantics::validate_mut(
                &mut slot.tensor,
                start,
                end,
                exclusive != 0,
            ) {
                return trap(runtime, &message);
            }
            match semantics::view_values(
                &slot.tensor,
                start,
                end,
                exclusive != 0,
            ) {
                Ok(values) => values,
                Err(message) => return trap(runtime, &message),
            }
        };
        let list = alloc_float_list(runtime, &values);
        if list == 0 {
            return 0;
        }
        let Ok(view_len) = i64::try_from(values.len()) else {
            return trap(runtime, "Tensor view length is too large");
        };
        let view_end = view_len.checked_sub(1).unwrap_or(-1);
        runtime.compute.windows.push(TensorWindowSlot {
            list,
            tensor,
            start,
            end,
            exclusive: exclusive != 0,
        });
        let view = runtime.heap.alloc_record(3);
        let _ = runtime.heap.record_set_int(view, 0, list);
        // Generic ViewMut marshalling sees a self-contained relative list. It
        // performs no Tensor policy; the shared window seam below receives the
        // logical index and original window facts.
        let _ = runtime.heap.record_set_int(view, 1, 0);
        let _ = runtime.heap.record_set_int(view, 2, view_end);
        view
    })
}

/// Ordinary JIT list writes are the marshalling edge for a compute ViewMut.
/// Keep them on the canonical Prelude setter; ordinary list views continue
/// through Collections' existing list setter below this hook.
pub(crate) fn try_set_list_f64(
    runtime: &mut JitRuntime,
    list: i64,
    index: i64,
    value: f64,
) -> bool {
    let Some(window_index) = runtime
        .compute
        .windows
        .iter()
        .position(|window| window.list == list)
    else {
        return false;
    };
    let window = runtime.compute.windows[window_index];
    let result = {
        let Some(slot) = slot_mut(runtime, window.tensor) else {
            runtime.set_trap("core.compute mutable view received an invalid Tensor handle");
            return true;
        };
        match semantics::window_set(
            &mut slot.tensor,
            window.start,
            window.end,
            window.exclusive,
            index,
            value,
        ) {
            Ok(()) => {
                let owner_values = semantics::tensor_values(&slot.tensor);
                let view_values = semantics::view_values(
                    &slot.tensor,
                    window.start,
                    window.end,
                    window.exclusive,
                );
                owner_values.and_then(|owner_values| {
                    view_values.map(|view_values| (owner_values, view_values))
                })
            }
            Err(message) => Err(message),
        }
    };
    match result {
        Ok((owner_values, view_values)) => {
            let owner_list = slot(runtime, window.tensor).map(|slot| slot.list);
            if let Some(owner_list) = owner_list {
                let _ = sync_float_list(runtime, owner_list, &owner_values);
            }
            let _ = sync_float_list(runtime, list, &view_values);
        }
        Err(message) => runtime.set_trap(&message),
    }
    true
}

pub(crate) fn try_get_list_f64(
    runtime: &mut JitRuntime,
    list: i64,
    index: i64,
) -> Option<f64> {
    let window = runtime
        .compute
        .windows
        .iter()
        .find(|window| window.list == list)
        .copied()?;
    let result = slot(runtime, window.tensor).map_or_else(
        || Err("core.compute mutable view received an invalid Tensor handle".to_string()),
        |slot| {
            semantics::window_get(
                &slot.tensor,
                window.start,
                window.end,
                window.exclusive,
                index,
            )
        },
    );
    match result {
        Ok(value) => Some(value),
        Err(message) => {
            runtime.set_trap(&message);
            Some(0.0)
        }
    }
}

host_fns! {
    struct ComputeHostFns;
    register: register_compute_symbols;
    declare: declare_compute_host_fns(module) {
        use cranelift_module::Module;
        let cc = module.target_config().default_call_conv;
        let mut sig_one = cranelift_codegen::ir::Signature::new(cc);
        sig_one.params.push(cranelift_codegen::ir::AbiParam::new(
            cranelift_codegen::ir::types::I64,
        ));
        sig_one.returns.push(cranelift_codegen::ir::AbiParam::new(
            cranelift_codegen::ir::types::I64,
        ));
        let mut sig_zero = cranelift_codegen::ir::Signature::new(cc);
        sig_zero.returns.push(cranelift_codegen::ir::AbiParam::new(
            cranelift_codegen::ir::types::I64,
        ));
        let mut sig_two = cranelift_codegen::ir::Signature::new(cc);
        for _ in 0..2 {
            sig_two
                .params
                .push(cranelift_codegen::ir::AbiParam::new(
                    cranelift_codegen::ir::types::I64,
                ));
        }
        sig_two.returns.push(cranelift_codegen::ir::AbiParam::new(
            cranelift_codegen::ir::types::I64,
        ));
        let mut sig_three_f64 = cranelift_codegen::ir::Signature::new(cc);
        sig_three_f64
            .params
            .push(cranelift_codegen::ir::AbiParam::new(
                cranelift_codegen::ir::types::I64,
            ));
        sig_three_f64
            .params
            .push(cranelift_codegen::ir::AbiParam::new(
                cranelift_codegen::ir::types::I64,
            ));
        sig_three_f64
            .params
            .push(cranelift_codegen::ir::AbiParam::new(
                cranelift_codegen::ir::types::F64,
            ));
        sig_three_f64
            .returns
            .push(cranelift_codegen::ir::AbiParam::new(
                cranelift_codegen::ir::types::I64,
            ));
        let mut sig_two_f64 = cranelift_codegen::ir::Signature::new(cc);
        sig_two_f64
            .params
            .push(cranelift_codegen::ir::AbiParam::new(
                cranelift_codegen::ir::types::I64,
            ));
        sig_two_f64
            .params
            .push(cranelift_codegen::ir::AbiParam::new(
                cranelift_codegen::ir::types::I64,
            ));
        sig_two_f64
            .params
            .push(cranelift_codegen::ir::AbiParam::new(
                cranelift_codegen::ir::types::F64,
            ));
        sig_two_f64
            .returns
            .push(cranelift_codegen::ir::AbiParam::new(
                cranelift_codegen::ir::types::I64,
            ));
        let mut sig_window = cranelift_codegen::ir::Signature::new(cc);
        for ty in [
            cranelift_codegen::ir::types::I64,
            cranelift_codegen::ir::types::I64,
            cranelift_codegen::ir::types::I64,
            cranelift_codegen::ir::types::I8,
        ] {
            sig_window
                .params
                .push(cranelift_codegen::ir::AbiParam::new(ty));
        }
        sig_window
            .returns
            .push(cranelift_codegen::ir::AbiParam::new(
                cranelift_codegen::ir::types::I64,
            ));
    }
    from_list: "jet_compute_from_list" => jet_jit_compute_from_list: sig_one;
    matrix: "jet_compute_matrix" => jet_jit_compute_matrix: sig_three_f64;
    zeros: "jet_compute_zeros" => jet_jit_compute_zeros: sig_one;
    ones: "jet_compute_ones" => jet_jit_compute_ones: sig_one;
    full: "jet_compute_full" => jet_jit_compute_full: sig_two_f64;
    eye: "jet_compute_eye" => jet_jit_compute_eye: sig_one;
    add: "jet_compute_add" => jet_jit_compute_add: sig_two;
    mul: "jet_compute_mul" => jet_jit_compute_mul: sig_two;
    sub: "jet_compute_sub" => jet_jit_compute_sub: sig_two;
    matmul: "jet_compute_matmul" => jet_jit_compute_matmul: sig_two;
    sum_axis: "jet_compute_sum_axis" => jet_jit_compute_sum_axis: sig_two;
    mse_loss: "jet_compute_mse_loss" => jet_jit_compute_mse_loss: sig_two;
    sgd_step: "jet_compute_sgd_step" => jet_jit_compute_sgd_step: sig_three_f64;
    serialize: "jet_compute_serialize" => jet_jit_compute_serialize: sig_one;
    deserialize: "jet_compute_deserialize" => jet_jit_compute_deserialize: sig_one;
    matmul_f32_tile: "jet_compute_matmul_f32_tile" => jet_jit_compute_matmul_f32_tile: sig_two;
    get: "jet_compute_get" => jet_jit_compute_get: sig_two;
    set: "jet_compute_set" => jet_jit_compute_set: sig_two_f64;
    shape: "jet_compute_tensor_shape" => jet_jit_compute_shape: sig_one;
    rank: "jet_compute_tensor_rank" => jet_jit_compute_rank: sig_one;
    numel: "jet_compute_tensor_numel" => jet_jit_compute_numel: sig_one;
    device: "jet_compute_tensor_device" => jet_jit_compute_device: sig_one;
    placement: "jet_compute_tensor_placement" => jet_jit_compute_placement: sig_one;
    profile_f32_strict: "jet_compute_profile_f32_strict" => jet_jit_compute_profile_show: sig_zero;
    profile_show: "jet_compute_profile_show" => jet_jit_compute_profile_show: sig_zero;
    copy: "jet_compute_copy" => jet_jit_compute_copy: sig_one;
    clone: "jet_compute_clone" => jet_jit_compute_clone: sig_one;
    drop_tensor: "jet_jit_compute_drop_tensor" => jet_jit_compute_drop_tensor: sig_one;
    drop_window: "jet_jit_compute_drop_window" => jet_jit_compute_drop_window: sig_one;
    tensor_to_list: "jet_compute_tensor_to_list" => jet_jit_compute_tensor_to_list: sig_one;
    slice: "jet_jit_compute_slice" => jet_jit_compute_slice: sig_window;
    view: "jet_jit_compute_view" => jet_jit_compute_view: sig_window;
    view_mut: "jet_jit_compute_view_mut" => jet_jit_compute_view_mut: sig_window;
}
