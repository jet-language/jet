//! Resident adapters for the shared compute Prelude.
//!
//! The JIT owns only handles and list marshalling. Tensor construction,
//! copying, view bounds, and writes stay in the same Prelude source used by
//! AOT and the interpreter (I9).

use crate::runtime_host::{jit_callable_parts, bind_jit_callable_handle, JitCallableSlot};
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
    pub(super) type Device = JetComputeDevice;
    pub(super) type Stream = JetComputeStream;
    pub(super) type Sparse = JetSparseCsr;
    pub(super) type Tape = std::sync::Arc<std::sync::Mutex<JetComputeTape>>;
    pub(super) type VjpState = JetComputeVjpState;

    pub(super) enum TransformResult {
        Gradient(Vec<Tensor>),
        ValueAndGradient {
            value: Tensor,
            gradients: Vec<Tensor>,
        },
        Vjp {
            value: Tensor,
            state: VjpState,
        },
        Jvp {
            value: Tensor,
            tangent: Tensor,
        },
    }

    pub(super) fn trace_inputs(inputs: Vec<Tensor>) -> (Tape, Vec<Tensor>) {
        jet_compute_trace_inputs(inputs)
    }

    pub(super) fn vjp_begin(value: Tensor, tape: Tape) -> VjpState {
        jet_compute_vjp_begin(value, tape)
    }

    pub(super) fn transform(
        method: &str,
        state: &VjpState,
        tangents: &[Tensor],
        targets: &[i64],
    ) -> Result<TransformResult, String> {
        match jet_compute_transform(method, state, tangents, targets) {
            Ok(JetComputeTransformResult::Gradient(values)) => {
                Ok(TransformResult::Gradient(values))
            }
            Ok(JetComputeTransformResult::ValueAndGradient { value, gradients }) => {
                Ok(TransformResult::ValueAndGradient { value, gradients })
            }
            Ok(JetComputeTransformResult::Vjp { value, state }) => {
                Ok(TransformResult::Vjp { value, state })
            }
            Ok(JetComputeTransformResult::Jvp { value, tangent }) => {
                Ok(TransformResult::Jvp { value, tangent })
            }
            Err(error) => Err(error.jet_show()),
        }
    }

    pub(super) fn nested_gradient(
        states: &[VjpState],
        targets: &[i64],
    ) -> Result<Vec<Vec<Tensor>>, String> {
        jet_compute_nested_gradient(states, targets).map_err(|error| error.jet_show())
    }

    pub(super) fn vjp_pull(
        state: &VjpState,
        seed: &Tensor,
        targets: &[i64],
    ) -> Result<Vec<Tensor>, String> {
        jet_compute_vjp_pull(state, seed, targets).map_err(|error| error.jet_show())
    }

    pub(super) fn vjp_gradient(
        state: &VjpState,
        targets: &[i64],
    ) -> Result<Vec<Tensor>, String> {
        let seed = jet_compute_gradient_seed(state).map_err(|error| error.jet_show())?;
        vjp_pull(state, &seed, targets)
    }

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

    pub(super) fn vec(len: i64, fill: f64) -> Result<Tensor, String> {
        jet_compute_vec(len, fill).map_err(|error| error.jet_show())
    }

    pub(super) fn reshape(tensor: &Tensor, shape: &[i64]) -> Result<Tensor, String> {
        jet_compute_reshape(tensor, &shape.to_vec()).map_err(|error| error.jet_show())
    }

    pub(super) fn device_cpu() -> Device {
        jet_compute_device_cpu()
    }

    pub(super) fn device_auto() -> Device {
        jet_compute_device_auto()
    }

    pub(super) fn device_word(device: Device) -> i64 {
        match device {
            JetComputeDevice::Auto => 0,
            JetComputeDevice::Cpu => 1,
        }
    }

    pub(super) fn device_from_word(word: i64) -> Option<Device> {
        match word {
            0 => Some(JetComputeDevice::Auto),
            1 => Some(JetComputeDevice::Cpu),
            _ => None,
        }
    }

    pub(super) fn on_device(tensor: &Tensor, device: Device) -> Result<Tensor, String> {
        jet_compute_on_device(tensor, device).map_err(|error| error.jet_show())
    }

    pub(super) fn broadcast_to(tensor: &Tensor, shape: &[i64]) -> Result<Tensor, String> {
        jet_compute_broadcast_to(tensor, &shape.to_vec()).map_err(|error| error.jet_show())
    }

    pub(super) fn transpose(tensor: &Tensor) -> Result<Tensor, String> {
        jet_compute_transpose(tensor).map_err(|error| error.jet_show())
    }

    pub(super) fn det(tensor: &Tensor) -> Result<f64, String> {
        jet_compute_det(tensor).map_err(|error| error.jet_show())
    }

    pub(super) fn inv(tensor: &Tensor) -> Result<Tensor, String> {
        jet_compute_inv(tensor).map_err(|error| error.jet_show())
    }

    pub(super) fn solve(left: &Tensor, right: &Tensor) -> Result<Tensor, String> {
        jet_compute_solve(left, right).map_err(|error| error.jet_show())
    }

    pub(super) fn fft(tensor: &Tensor) -> Result<Tensor, String> {
        jet_compute_fft(tensor).map_err(|error| error.jet_show())
    }

    pub(super) fn stream_new() -> Stream {
        jet_compute_stream_new()
    }

    pub(super) fn stream_sync(stream: &Stream) -> Result<(), String> {
        jet_compute_stream_sync(stream).map_err(|error| error.jet_show())
    }

    pub(super) fn stream_show(stream: &Stream) -> String {
        jet_compute_stream_show(stream)
    }

    pub(super) fn transfer(tensor: &Tensor, device: Device) -> Result<Tensor, String> {
        jet_compute_transfer(tensor, device).map_err(|error| error.jet_show())
    }

    pub(super) fn transfer_show(tensor: &Tensor) -> String {
        jet_compute_transfer_show(tensor)
    }

    pub(super) fn kernel_bounds_ok(shape: &[i64], indices: &[i64]) -> Result<bool, String> {
        jet_compute_kernel_bounds_ok(shape, indices).map_err(|error| error.jet_show())
    }

    pub(super) fn to_sparse(tensor: &Tensor) -> Result<Sparse, String> {
        jet_compute_to_sparse(tensor).map_err(|error| error.jet_show())
    }

    pub(super) fn sparse_nnz(sparse: &Sparse) -> i64 {
        jet_compute_sparse_nnz(sparse)
    }

    pub(super) fn sparse_mv(sparse: &Sparse, vector: &Tensor) -> Result<Tensor, String> {
        jet_compute_sparse_mv(sparse, vector).map_err(|error| error.jet_show())
    }

    pub(super) fn sparse_show(sparse: &Sparse) -> String {
        jet_compute_sparse_show(sparse)
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

    pub(super) fn div(left: &Tensor, right: &Tensor) -> Result<Tensor, String> {
        jet_compute_binary("div", left, right).map_err(|error| error.jet_show())
    }

    pub(super) fn maximum(left: &Tensor, right: &Tensor) -> Result<Tensor, String> {
        jet_compute_binary("maximum", left, right).map_err(|error| error.jet_show())
    }

    pub(super) fn minimum(left: &Tensor, right: &Tensor) -> Result<Tensor, String> {
        jet_compute_binary("minimum", left, right).map_err(|error| error.jet_show())
    }

    pub(super) fn unary(op: &str, tensor: &Tensor) -> Result<Tensor, String> {
        jet_compute_unary(op, tensor).map_err(|error| error.jet_show())
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
    streams: Vec<Option<semantics::Stream>>,
    sparse: Vec<Option<semantics::Sparse>>,
    vjp_states: Vec<Option<semantics::VjpState>>,
}

impl ComputeState {
    pub(crate) fn clear(&mut self) {
        self.windows.clear();
        self.slots.clear();
        self.streams.clear();
        self.sparse.clear();
        self.vjp_states.clear();
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

fn stream<'a>(runtime: &'a JitRuntime, handle: i64) -> Option<&'a semantics::Stream> {
    tensor_index(handle)
        .and_then(|index| runtime.compute.streams.get(index))
        .and_then(Option::as_ref)
}

fn sparse<'a>(runtime: &'a JitRuntime, handle: i64) -> Option<&'a semantics::Sparse> {
    tensor_index(handle)
        .and_then(|index| runtime.compute.sparse.get(index))
        .and_then(Option::as_ref)
}

fn vjp_state<'a>(runtime: &'a JitRuntime, handle: i64) -> Option<&'a semantics::VjpState> {
    tensor_index(handle)
        .and_then(|index| runtime.compute.vjp_states.get(index))
        .and_then(Option::as_ref)
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

fn alloc_record_words(runtime: &mut JitRuntime, values: &[i64], context: &str) -> i64 {
    let record = runtime.heap.alloc_record(values.len());
    for (index, &value) in values.iter().enumerate() {
        let Ok(index) = i64::try_from(index) else {
            runtime.set_trap(&format!("{context} field index overflow"));
            return 0;
        };
        if runtime.heap.record_set_int(record, index, value).is_none() {
            runtime.set_trap(&format!("{context} record allocation failed"));
            return 0;
        }
    }
    record
}

fn alloc_tensor_record(
    runtime: &mut JitRuntime,
    tensors: &[semantics::Tensor],
    context: &str,
) -> i64 {
    let handles = tensors
        .iter()
        .cloned()
        .map(|tensor| alloc_tensor(runtime, tensor))
        .collect::<Vec<_>>();
    if handles.iter().any(|handle| *handle == 0) {
        return 0;
    }
    alloc_record_words(runtime, &handles, context)
}

fn read_tensor_list(runtime: &JitRuntime, handle: i64) -> Option<Vec<i64>> {
    let values = read_int_list(runtime, handle)?;
    values
        .iter()
        .all(|value| slot(runtime, *value).is_some())
        .then_some(values)
}

fn read_record_words(runtime: &JitRuntime, handle: i64, count: usize) -> Option<Vec<i64>> {
    (0..count)
        .map(|index| runtime.heap.record_get_int(handle, index as i64))
        .collect()
}

fn invoke_callable(slot: JitCallableSlot, args: &[i64]) -> i64 {
    // SAFETY: the callable binder records the Cranelift signature shape and
    // this adapter only invokes Tensor-valued functions after sema has proved
    // their arity. The resident function-value ABI is C-compatible on the JIT
    // target, with the environment word prepended for captured values.
    unsafe {
        if slot.has_env {
            match args {
                [] => {
                    let callback: unsafe extern "C" fn(i64) -> i64 =
                        std::mem::transmute(slot.fn_ptr as usize);
                    callback(slot.env)
                }
                [a] => {
                    let callback: unsafe extern "C" fn(i64, i64) -> i64 =
                        std::mem::transmute(slot.fn_ptr as usize);
                    callback(slot.env, *a)
                }
                [a, b] => {
                    let callback: unsafe extern "C" fn(i64, i64, i64) -> i64 =
                        std::mem::transmute(slot.fn_ptr as usize);
                    callback(slot.env, *a, *b)
                }
                [a, b, c] => {
                    let callback: unsafe extern "C" fn(i64, i64, i64, i64) -> i64 =
                        std::mem::transmute(slot.fn_ptr as usize);
                    callback(slot.env, *a, *b, *c)
                }
                [a, b, c, d] => {
                    let callback: unsafe extern "C" fn(i64, i64, i64, i64, i64) -> i64 =
                        std::mem::transmute(slot.fn_ptr as usize);
                    callback(slot.env, *a, *b, *c, *d)
                }
                [a, b, c, d, e] => {
                    let callback: unsafe extern "C" fn(i64, i64, i64, i64, i64, i64) -> i64 =
                        std::mem::transmute(slot.fn_ptr as usize);
                    callback(slot.env, *a, *b, *c, *d, *e)
                }
                [a, b, c, d, e, f] => {
                    let callback: unsafe extern "C" fn(i64, i64, i64, i64, i64, i64, i64) -> i64 =
                        std::mem::transmute(slot.fn_ptr as usize);
                    callback(slot.env, *a, *b, *c, *d, *e, *f)
                }
                _ => 0,
            }
        } else {
            match args {
                [] => {
                    let callback: unsafe extern "C" fn() -> i64 =
                        std::mem::transmute(slot.fn_ptr as usize);
                    callback()
                }
                [a] => {
                    let callback: unsafe extern "C" fn(i64) -> i64 =
                        std::mem::transmute(slot.fn_ptr as usize);
                    callback(*a)
                }
                [a, b] => {
                    let callback: unsafe extern "C" fn(i64, i64) -> i64 =
                        std::mem::transmute(slot.fn_ptr as usize);
                    callback(*a, *b)
                }
                [a, b, c] => {
                    let callback: unsafe extern "C" fn(i64, i64, i64) -> i64 =
                        std::mem::transmute(slot.fn_ptr as usize);
                    callback(*a, *b, *c)
                }
                [a, b, c, d] => {
                    let callback: unsafe extern "C" fn(i64, i64, i64, i64) -> i64 =
                        std::mem::transmute(slot.fn_ptr as usize);
                    callback(*a, *b, *c, *d)
                }
                [a, b, c, d, e] => {
                    let callback: unsafe extern "C" fn(i64, i64, i64, i64, i64) -> i64 =
                        std::mem::transmute(slot.fn_ptr as usize);
                    callback(*a, *b, *c, *d, *e)
                }
                [a, b, c, d, e, f] => {
                    let callback: unsafe extern "C" fn(i64, i64, i64, i64, i64, i64) -> i64 =
                        std::mem::transmute(slot.fn_ptr as usize);
                    callback(*a, *b, *c, *d, *e, *f)
                }
                _ => 0,
            }
        }
    }
}

fn transform_failure(runtime: &mut JitRuntime, message: &str) -> i64 {
    runtime.set_trap(message);
    0
}

fn alloc_vjp_run(
    runtime: &mut JitRuntime,
    value: semantics::Tensor,
    state: semantics::VjpState,
    targets: &[i64],
    targets_handle: i64,
) -> i64 {
    let gradients = match semantics::vjp_gradient(&state, targets) {
        Ok(values) => values,
        Err(message) => return transform_failure(runtime, &message),
    };
    runtime.compute.vjp_states.push(Some(state));
    let state_handle = runtime.compute.vjp_states.len() as i64;
    let pull_env = alloc_record_words(
        runtime,
        &[state_handle, targets_handle],
        "core.compute.vjp.pull",
    );
    if pull_env == 0 {
        return 0;
    }
    let pull = bind_jit_callable_handle(
        runtime,
        jet_jit_compute_vjp_pull as usize as i64,
        pull_env,
        true,
    );
    if pull == 0 {
        return 0;
    }
    let value_handle = alloc_tensor(runtime, value);
    let gradients_handle = alloc_tensor_record(runtime, &gradients, "core.compute.vjp.grads");
    if value_handle == 0 || gradients_handle == 0 {
        return 0;
    }
    alloc_record_words(
        runtime,
        &[value_handle, pull, gradients_handle],
        "core.compute.vjp",
    )
}

fn run_transform(
    runtime: &mut JitRuntime,
    base_handle: i64,
    inputs_handle: i64,
    targets_handle: i64,
    method: &str,
    base_arity: usize,
    result_fields: usize,
) -> i64 {
    let Some(callable) = jit_callable_parts(runtime, base_handle) else {
        return transform_failure(runtime, "core.compute transform received an invalid function");
    };
    let Some(inputs) = read_tensor_list(runtime, inputs_handle) else {
        return transform_failure(runtime, "core.compute transform expects Tensor arguments");
    };
    let Some(targets) = read_int_list(runtime, targets_handle) else {
        return transform_failure(runtime, "core.compute transform expects integer targets");
    };
    let expected_inputs = if method == "jvp" {
        base_arity.saturating_mul(2)
    } else {
        base_arity
    };
    if inputs.len() != expected_inputs {
        return transform_failure(runtime, "core.compute transform argument count mismatch");
    }
    if base_arity > 6 {
        return transform_failure(runtime, "core.compute transform function arity exceeds the resident ABI");
    }
    let primal_handles = &inputs[..base_arity];
    let tangent_handles = if method == "jvp" {
        &inputs[base_arity..]
    } else {
        &[]
    };
    let input_tensors = match primal_handles
        .iter()
        .map(|handle| slot(runtime, *handle).map(|slot| slot.tensor.clone()))
        .collect::<Option<Vec<_>>>()
    {
        Some(values) => values,
        None => return transform_failure(runtime, "core.compute transform received an invalid Tensor"),
    };
    let tangent_tensors = match tangent_handles
        .iter()
        .map(|handle| slot(runtime, *handle).map(|slot| slot.tensor.clone()))
        .collect::<Option<Vec<_>>>()
    {
        Some(values) => values,
        None => return transform_failure(runtime, "core.compute.jvp received an invalid tangent Tensor"),
    };
    let (tape, traced) = semantics::trace_inputs(input_tensors);
    let mut originals = Vec::with_capacity(primal_handles.len());
    for (handle, traced_tensor) in primal_handles.iter().zip(traced.iter()) {
        let Some(slot) = slot_mut(runtime, *handle) else {
            return transform_failure(runtime, "core.compute transform received an invalid Tensor");
        };
        originals.push((*handle, slot.tensor.clone()));
        slot.tensor = traced_tensor.clone();
    }
    let output_handle = invoke_callable(callable, primal_handles);
    let output_record = if output_handle != 0 && result_fields != 0 {
        read_record_words(runtime, output_handle, result_fields)
    } else {
        None
    };
    let output_tensor = if output_handle != 0 && result_fields == 0 {
        slot(runtime, output_handle).map(|slot| slot.tensor.clone())
    } else {
        None
    };
    for (handle, original) in originals {
        if let Some(slot) = slot_mut(runtime, handle) {
            slot.tensor = original;
        }
    }
    if output_handle == 0 {
        return transform_failure(runtime, "core.compute transform function returned no value");
    }
    let targets = targets.as_slice();
    if result_fields != 0 {
        if method != "gradient" {
            return transform_failure(runtime, "core.compute transform requires a Tensor result");
        }
        let Some(fields) = output_record else {
            return transform_failure(runtime, "core.compute.gradient returned an invalid tuple");
        };
        let states = match fields
            .iter()
            .map(|handle| slot(runtime, *handle).map(|slot| semantics::vjp_begin(slot.tensor.clone(), tape.clone())))
            .collect::<Option<Vec<_>>>()
        {
            Some(states) => states,
            None => return transform_failure(runtime, "core.compute.gradient tuple field is not a Tensor"),
        };
        let nested = match semantics::nested_gradient(&states, targets) {
            Ok(values) => values,
            Err(message) => return transform_failure(runtime, &message),
        };
        let mut outer_handles = Vec::with_capacity(nested.len());
        for values in nested {
            let handle = alloc_tensor_record(runtime, &values, "core.compute.gradient");
            if handle == 0 {
                return 0;
            }
            outer_handles.push(handle);
        }
        return alloc_record_words(runtime, &outer_handles, "core.compute.gradient");
    }
    let Some(output_tensor) = output_tensor else {
        return transform_failure(runtime, "core.compute transform returned an invalid Tensor");
    };
    let state = semantics::vjp_begin(output_tensor, tape);
    let transform = match semantics::transform(method, &state, &tangent_tensors, targets) {
        Ok(result) => result,
        Err(message) => return transform_failure(runtime, &message),
    };
    match transform {
        semantics::TransformResult::Gradient(values) => {
            alloc_tensor_record(runtime, &values, "core.compute.gradient")
        }
        semantics::TransformResult::ValueAndGradient { value, gradients } => {
            let value_handle = alloc_tensor(runtime, value);
            let gradients_handle = alloc_tensor_record(
                runtime,
                &gradients,
                "core.compute.value_and_gradient",
            );
            if value_handle == 0 || gradients_handle == 0 {
                0
            } else {
                alloc_record_words(
                    runtime,
                    &[value_handle, gradients_handle],
                    "core.compute.value_and_gradient",
                )
            }
        }
        semantics::TransformResult::Vjp { value, state } => {
            alloc_vjp_run(runtime, value, state, targets, targets_handle)
        }
        semantics::TransformResult::Jvp { value, tangent } => {
            let value_handle = alloc_tensor(runtime, value);
            let tangent_handle = alloc_tensor(runtime, tangent);
            if value_handle == 0 || tangent_handle == 0 {
                0
            } else {
                alloc_record_words(
                    runtime,
                    &[value_handle, tangent_handle],
                    "core.compute.jvp",
                )
            }
        }
    }
}

fn run_transform_factory(runtime: &mut JitRuntime, env: i64, inputs: &[i64]) -> i64 {
    let Some(fields) = read_record_words(runtime, env, 5) else {
        return transform_failure(runtime, "core.compute transform environment is invalid");
    };
    let Some(method) = runtime.heap.clone_string(fields[2]) else {
        return transform_failure(runtime, "core.compute transform method is invalid");
    };
    let Ok(base_arity) = usize::try_from(fields[3]) else {
        return transform_failure(runtime, "core.compute transform arity is invalid");
    };
    let Ok(result_fields) = usize::try_from(fields[4]) else {
        return transform_failure(runtime, "core.compute transform result shape is invalid");
    };
    let inputs_handle = alloc_int_list(runtime, inputs);
    if inputs_handle == 0 {
        return 0;
    }
    run_transform(
        runtime,
        fields[0],
        inputs_handle,
        fields[1],
        &method,
        base_arity,
        result_fields,
    )
}

extern "C" fn jet_jit_compute_transform_factory_0(env: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| run_transform_factory(runtime, env, &[]))
}

extern "C" fn jet_jit_compute_transform_factory_1(env: i64, a: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| run_transform_factory(runtime, env, &[a]))
}

extern "C" fn jet_jit_compute_transform_factory_2(env: i64, a: i64, b: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| run_transform_factory(runtime, env, &[a, b]))
}

extern "C" fn jet_jit_compute_transform_factory_3(env: i64, a: i64, b: i64, c: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| run_transform_factory(runtime, env, &[a, b, c]))
}

extern "C" fn jet_jit_compute_transform_factory_4(
    env: i64,
    a: i64,
    b: i64,
    c: i64,
    d: i64,
) -> i64 {
    Concurrency::with_runtime_mut(|runtime| run_transform_factory(runtime, env, &[a, b, c, d]))
}

extern "C" fn jet_jit_compute_transform_factory_5(
    env: i64,
    a: i64,
    b: i64,
    c: i64,
    d: i64,
    e: i64,
) -> i64 {
    Concurrency::with_runtime_mut(|runtime| run_transform_factory(runtime, env, &[a, b, c, d, e]))
}

extern "C" fn jet_jit_compute_transform_factory_6(
    env: i64,
    a: i64,
    b: i64,
    c: i64,
    d: i64,
    e: i64,
    f: i64,
) -> i64 {
    Concurrency::with_runtime_mut(|runtime| run_transform_factory(runtime, env, &[a, b, c, d, e, f]))
}

extern "C" fn jet_jit_compute_vjp_pull(env: i64, seed: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(fields) = read_record_words(runtime, env, 2) else {
            return transform_failure(runtime, "core.compute.vjp.pull environment is invalid");
        };
        let Some(state) = vjp_state(runtime, fields[0]).cloned() else {
            return transform_failure(runtime, "core.compute.vjp.pull state is invalid");
        };
        let Some(seed) = slot(runtime, seed).map(|slot| slot.tensor.clone()) else {
            return transform_failure(runtime, "core.compute.vjp.pull seed is invalid");
        };
        let Some(targets) = read_int_list(runtime, fields[1]) else {
            return transform_failure(runtime, "core.compute.vjp.pull targets are invalid");
        };
        let values = match semantics::vjp_pull(&state, &seed, &targets) {
            Ok(values) => values,
            Err(message) => return transform_failure(runtime, &message),
        };
        alloc_tensor_record(runtime, &values, "core.compute.vjp.pull")
    })
}

extern "C" fn jet_jit_compute_transform(
    base: i64,
    inputs: i64,
    targets: i64,
    method: i64,
    base_arity: i64,
    result_fields: i64,
) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(method) = runtime.heap.clone_string(method) else {
            return transform_failure(runtime, "core.compute transform method is invalid");
        };
        let Ok(base_arity) = usize::try_from(base_arity) else {
            return transform_failure(runtime, "core.compute transform arity is invalid");
        };
        let Ok(result_fields) = usize::try_from(result_fields) else {
            return transform_failure(runtime, "core.compute transform result shape is invalid");
        };
        if inputs != 0 {
            return run_transform(
                runtime,
                base,
                inputs,
                targets,
                &method,
                base_arity,
                result_fields,
            );
        }
        let adapter_arity = if method == "jvp" {
            base_arity.saturating_mul(2)
        } else {
            base_arity
        };
        let fn_ptr = match adapter_arity {
            0 => jet_jit_compute_transform_factory_0 as usize as i64,
            1 => jet_jit_compute_transform_factory_1 as usize as i64,
            2 => jet_jit_compute_transform_factory_2 as usize as i64,
            3 => jet_jit_compute_transform_factory_3 as usize as i64,
            4 => jet_jit_compute_transform_factory_4 as usize as i64,
            5 => jet_jit_compute_transform_factory_5 as usize as i64,
            6 => jet_jit_compute_transform_factory_6 as usize as i64,
            _ => return transform_failure(runtime, "core.compute transform function arity exceeds the resident ABI"),
        };
        let env = alloc_record_words(
            runtime,
            &[base, targets, method, base_arity as i64, result_fields as i64],
            "core.compute transform",
        );
        if env == 0 {
            return 0;
        }
        bind_jit_callable_handle(runtime, fn_ptr, env, true)
    })
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

extern "C" fn jet_jit_compute_vec(len: i64, fill: f64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| match semantics::vec(len, fill) {
        Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
        Err(message) => result_err_msg(&message),
    })
}

extern "C" fn jet_jit_compute_reshape(tensor: i64, shape: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(shape) = read_int_list(runtime, shape) else {
            return result_err_msg("core.compute.reshape expects an integer shape list");
        };
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return result_err_msg("core.compute.reshape received an invalid Tensor handle");
        };
        match semantics::reshape(&tensor, &shape) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

extern "C" fn jet_jit_compute_device_cpu() -> i64 {
    semantics::device_word(semantics::device_cpu())
}

extern "C" fn jet_jit_compute_device_auto() -> i64 {
    semantics::device_word(semantics::device_auto())
}

extern "C" fn jet_jit_compute_on_device(tensor: i64, device: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(device) = semantics::device_from_word(device) else {
            return result_err_msg("core.compute.on_device received an invalid device");
        };
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return result_err_msg("core.compute.on_device received an invalid Tensor handle");
        };
        match semantics::on_device(&tensor, device) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

extern "C" fn jet_jit_compute_broadcast_to(tensor: i64, shape: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(shape) = read_int_list(runtime, shape) else {
            return result_err_msg("core.compute.broadcast_to expects an integer shape list");
        };
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return result_err_msg("core.compute.broadcast_to received an invalid Tensor handle");
        };
        match semantics::broadcast_to(&tensor, &shape) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

extern "C" fn jet_jit_compute_transpose(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return result_err_msg("core.compute.transpose received an invalid Tensor handle");
        };
        match semantics::transpose(&tensor) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

extern "C" fn jet_jit_compute_det(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return result_err_msg("core.compute.det received an invalid Tensor handle");
        };
        match semantics::det(&tensor) {
            Ok(value) => result_ok(value.to_bits()),
            Err(message) => result_err_msg(&message),
        }
    })
}

extern "C" fn jet_jit_compute_inv(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return result_err_msg("core.compute.inv received an invalid Tensor handle");
        };
        match semantics::inv(&tensor) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

extern "C" fn jet_jit_compute_solve(left: i64, right: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some((left, right)) = clone_tensor_pair(runtime, left, right) else {
            return result_err_msg("core.compute.solve received an invalid Tensor handle");
        };
        match semantics::solve(&left, &right) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

extern "C" fn jet_jit_compute_fft(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return result_err_msg("core.compute.fft received an invalid Tensor handle");
        };
        match semantics::fft(&tensor) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

extern "C" fn jet_jit_compute_stream_new() -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        runtime.compute.streams.push(Some(semantics::stream_new()));
        runtime.compute.streams.len() as i64
    })
}

extern "C" fn jet_jit_compute_stream_sync(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(stream) = stream(runtime, handle) else {
            return result_err_msg("core.compute.stream_sync received an invalid stream handle");
        };
        match semantics::stream_sync(stream) {
            Ok(()) => result_ok(0),
            Err(message) => result_err_msg(&message),
        }
    })
}

extern "C" fn jet_jit_compute_stream_show(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(stream) = stream(runtime, handle) else {
            runtime.set_trap("core.compute.stream_show received an invalid stream handle");
            return 0;
        };
        runtime.heap.alloc_string(semantics::stream_show(stream))
    })
}

extern "C" fn jet_jit_compute_transfer(tensor: i64, device: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(device) = semantics::device_from_word(device) else {
            return result_err_msg("core.compute.transfer received an invalid device");
        };
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return result_err_msg("core.compute.transfer received an invalid Tensor handle");
        };
        match semantics::transfer(&tensor, device) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

extern "C" fn jet_jit_compute_transfer_show(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            runtime.set_trap("core.compute.transfer_show received an invalid Tensor handle");
            return 0;
        };
        runtime.heap.alloc_string(semantics::transfer_show(&tensor))
    })
}

extern "C" fn jet_jit_compute_kernel_bounds_ok(shape: i64, indices: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(shape) = read_int_list(runtime, shape) else {
            return result_err_msg("core.compute.kernel_bounds_ok expects an integer shape list");
        };
        let Some(indices) = read_int_list(runtime, indices) else {
            return result_err_msg("core.compute.kernel_bounds_ok expects an integer index list");
        };
        match semantics::kernel_bounds_ok(&shape, &indices) {
            Ok(value) => result_ok(u64::from(value)),
            Err(message) => result_err_msg(&message),
        }
    })
}

extern "C" fn jet_jit_compute_to_sparse(tensor: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return result_err_msg("core.compute.to_sparse received an invalid Tensor handle");
        };
        match semantics::to_sparse(&tensor) {
            Ok(sparse) => {
                runtime.compute.sparse.push(Some(sparse));
                result_ok(runtime.compute.sparse.len() as u64)
            }
            Err(message) => result_err_msg(&message),
        }
    })
}

extern "C" fn jet_jit_compute_sparse_nnz(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| match sparse(runtime, handle) {
        Some(sparse) => semantics::sparse_nnz(sparse),
        None => trap(runtime, "core.compute.sparse_nnz received an invalid sparse handle"),
    })
}

extern "C" fn jet_jit_compute_sparse_mv(sparse_handle: i64, vector: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(sparse) = sparse(runtime, sparse_handle).cloned() else {
            return result_err_msg("core.compute.sparse_mv received an invalid sparse handle");
        };
        let Some(vector) = slot(runtime, vector).map(|slot| slot.tensor.clone()) else {
            return result_err_msg("core.compute.sparse_mv received an invalid Tensor handle");
        };
        match semantics::sparse_mv(&sparse, &vector) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

extern "C" fn jet_jit_compute_sparse_show(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(sparse) = sparse(runtime, handle) else {
            runtime.set_trap("core.compute.sparse_show received an invalid sparse handle");
            return 0;
        };
        runtime.heap.alloc_string(semantics::sparse_show(sparse))
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

fn compute_binary_op(
    left: i64,
    right: i64,
    name: &str,
    op: fn(&semantics::Tensor, &semantics::Tensor) -> Result<semantics::Tensor, String>,
) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some((left, right)) = clone_tensor_pair(runtime, left, right) else {
            return result_err_msg(&format!("core.compute.{name} received an invalid Tensor handle"));
        };
        match op(&left, &right) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

extern "C" fn jet_jit_compute_div(left: i64, right: i64) -> i64 {
    compute_binary_op(left, right, "div", semantics::div)
}

extern "C" fn jet_jit_compute_maximum(left: i64, right: i64) -> i64 {
    compute_binary_op(left, right, "maximum", semantics::maximum)
}

extern "C" fn jet_jit_compute_minimum(left: i64, right: i64) -> i64 {
    compute_binary_op(left, right, "minimum", semantics::minimum)
}

fn compute_unary_op(tensor: i64, op: &str, name: &str) -> i64 {
    Concurrency::with_runtime_mut(|runtime| {
        let Some(tensor) = slot(runtime, tensor).map(|slot| slot.tensor.clone()) else {
            return result_err_msg(&format!("core.compute.{name} received an invalid Tensor handle"));
        };
        match semantics::unary(op, &tensor) {
            Ok(tensor) => result_ok(alloc_tensor(runtime, tensor) as u64),
            Err(message) => result_err_msg(&message),
        }
    })
}

extern "C" fn jet_jit_compute_negate(tensor: i64) -> i64 {
    compute_unary_op(tensor, "negate", "negate")
}

extern "C" fn jet_jit_compute_abs(tensor: i64) -> i64 {
    compute_unary_op(tensor, "abs", "abs")
}

extern "C" fn jet_jit_compute_exp(tensor: i64) -> i64 {
    compute_unary_op(tensor, "exp", "exp")
}

extern "C" fn jet_jit_compute_log(tensor: i64) -> i64 {
    compute_unary_op(tensor, "log", "log")
}

extern "C" fn jet_jit_compute_sqrt(tensor: i64) -> i64 {
    compute_unary_op(tensor, "sqrt", "sqrt")
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
        let mut sig_one_f64 = cranelift_codegen::ir::Signature::new(cc);
        sig_one_f64
            .params
            .push(cranelift_codegen::ir::AbiParam::new(
                cranelift_codegen::ir::types::I64,
            ));
        sig_one_f64
            .params
            .push(cranelift_codegen::ir::AbiParam::new(
                cranelift_codegen::ir::types::F64,
            ));
        sig_one_f64
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
        let mut sig_transform = cranelift_codegen::ir::Signature::new(cc);
        for _ in 0..6 {
            sig_transform
                .params
                .push(cranelift_codegen::ir::AbiParam::new(
                    cranelift_codegen::ir::types::I64,
                ));
        }
        sig_transform
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
    vec: "jet_compute_vec" => jet_jit_compute_vec: sig_one_f64;
    zeros: "jet_compute_zeros" => jet_jit_compute_zeros: sig_one;
    ones: "jet_compute_ones" => jet_jit_compute_ones: sig_one;
    full: "jet_compute_full" => jet_jit_compute_full: sig_one_f64;
    eye: "jet_compute_eye" => jet_jit_compute_eye: sig_one;
    reshape: "jet_compute_reshape" => jet_jit_compute_reshape: sig_two;
    add: "jet_compute_add" => jet_jit_compute_add: sig_two;
    mul: "jet_compute_mul" => jet_jit_compute_mul: sig_two;
    sub: "jet_compute_sub" => jet_jit_compute_sub: sig_two;
    div: "jet_compute_div" => jet_jit_compute_div: sig_two;
    maximum: "jet_compute_maximum" => jet_jit_compute_maximum: sig_two;
    minimum: "jet_compute_minimum" => jet_jit_compute_minimum: sig_two;
    negate: "jet_compute_negate" => jet_jit_compute_negate: sig_one;
    abs: "jet_compute_abs" => jet_jit_compute_abs: sig_one;
    exp: "jet_compute_exp" => jet_jit_compute_exp: sig_one;
    log: "jet_compute_log" => jet_jit_compute_log: sig_one;
    sqrt: "jet_compute_sqrt" => jet_jit_compute_sqrt: sig_one;
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
    device_cpu: "jet_compute_device_cpu" => jet_jit_compute_device_cpu: sig_zero;
    device_auto: "jet_compute_device_auto" => jet_jit_compute_device_auto: sig_zero;
    on_device: "jet_compute_on_device" => jet_jit_compute_on_device: sig_two;
    broadcast_to: "jet_compute_broadcast_to" => jet_jit_compute_broadcast_to: sig_two;
    transpose: "jet_compute_transpose" => jet_jit_compute_transpose: sig_one;
    det: "jet_compute_det" => jet_jit_compute_det: sig_one;
    inv: "jet_compute_inv" => jet_jit_compute_inv: sig_one;
    solve: "jet_compute_solve" => jet_jit_compute_solve: sig_two;
    fft: "jet_compute_fft" => jet_jit_compute_fft: sig_one;
    stream_new: "jet_compute_stream_new" => jet_jit_compute_stream_new: sig_zero;
    stream_sync: "jet_compute_stream_sync" => jet_jit_compute_stream_sync: sig_one;
    stream_show: "jet_compute_stream_show" => jet_jit_compute_stream_show: sig_one;
    transfer: "jet_compute_transfer" => jet_jit_compute_transfer: sig_two;
    transfer_show: "jet_compute_transfer_show" => jet_jit_compute_transfer_show: sig_one;
    kernel_bounds_ok: "jet_compute_kernel_bounds_ok" => jet_jit_compute_kernel_bounds_ok: sig_two;
    to_sparse: "jet_compute_to_sparse" => jet_jit_compute_to_sparse: sig_one;
    sparse_nnz: "jet_compute_sparse_nnz" => jet_jit_compute_sparse_nnz: sig_one;
    sparse_mv: "jet_compute_sparse_mv" => jet_jit_compute_sparse_mv: sig_two;
    sparse_show: "jet_compute_sparse_show" => jet_jit_compute_sparse_show: sig_one;
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
    transform: "jet_compute_transform" => jet_jit_compute_transform: sig_transform;
}
