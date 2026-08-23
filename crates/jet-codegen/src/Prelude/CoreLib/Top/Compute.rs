// ── D-COMPUTE1=D / D-COMPUTE-TYPE1=D / D-COMPUTE-PLACE1=D (#443) ─────────────
// One Core compute family. `Tensor` owns ranked multidimensional storage on the
// selected CPU-oracle or explicit Metal ability; views retain the backing allocation and its
// strides. Mutable access requires the sema-proved exclusive ViewMut path;
// shared writes fail closed instead of copying or pretending to update an alias.
// Explicit Tensor copies materialize logical values into fresh backing storage.
// Engines only marshal into these Prelude symbols (I9).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetComputeDevice {
    Auto,
    Cpu,
    Metal,
}

const MAX_TENSOR_ELEMENTS: usize = 16 * 1024 * 1024;
const CPU_ORACLE_BACKEND: &str = "cpu-oracle";
const CPU_ORACLE_VERSION: &str = "builtin";
const CPU_ORACLE_CACHE: &str = "none";
const METAL_BACKEND: &str = "metal";
const METAL_VERSION: &str = "system";
const METAL_CACHE: &str = "runtime";
const CPU_ORACLE_F64_PROFILE: &str = "F64Strict+Reproducible";
const CPU_ORACLE_F32_PROFILE: &str = "F32Strict+Reproducible";
const CPU_ORACLE_F64_CAPABILITIES: &[&str] = &[
    "ranked-storage",
    "strided-view",
    "checked-bounds",
    "reproducible-reduction",
    "differential-oracle",
];
const CPU_ORACLE_F32_CAPABILITIES: &[&str] = &[
    "ranked-storage",
    "strided-view",
    "checked-bounds",
    "f32-arithmetic",
    "cpu-simd-dispatch",
    "simd-tail",
    "blocked-matmul",
    "differential-oracle",
];
const METAL_F32_CAPABILITIES: &[&str] = &[
    "ranked-storage",
    "strided-view",
    "checked-bounds",
    "f32-arithmetic",
    "reproducible-reduction",
    "elementwise",
    "matmul",
    "device-buffer",
    "stream",
    "differential-oracle",
];

// D-COMPUTE-BACKEND1=D / #1145: the Metal bridge is a Prelude-owned native
// adapter. It stages canonical F32 values into shared Metal buffers, launches
// checked kernels, and reads the result back into the Tensor owner. No host
// engine selects a kernel or supplies a fallback policy.
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod jet_compute_metal {
    use super::JetComputeError;
    use std::ffi::CString;

    type Obj = usize;

    const STATUS_COMPLETED: Obj = 4;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Size {
        width: usize,
        height: usize,
        depth: usize,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Params {
        count: u32,
        rows: u32,
        inner: u32,
        cols: u32,
        op: u32,
        scalar: f32,
    }

    const SHADER: &str = r#"
#include <metal_stdlib>
using namespace metal;
#pragma clang fp contract(off)

struct JetParams {
    uint count;
    uint rows;
    uint inner;
    uint cols;
    uint op;
    float scalar;
};

kernel void jet_binary(
    device const float* a [[buffer(0)]],
    device const float* b [[buffer(1)]],
    device float* out [[buffer(2)]],
    constant JetParams& p [[buffer(3)]],
    uint id [[thread_position_in_grid]]) {
    if (id >= p.count) return;
    float left = a[id];
    float right = b[id];
    switch (p.op) {
        case 0: out[id] = left + right; break;
        case 1: out[id] = left * right; break;
        case 2: out[id] = left - right; break;
        case 3: out[id] = left / right; break;
        case 4: out[id] = max(left, right); break;
        case 5: out[id] = min(left, right); break;
        default: out[id] = 0.0f; break;
    }
}

kernel void jet_unary(
    device const float* a [[buffer(0)]],
    device float* out [[buffer(1)]],
    constant JetParams& p [[buffer(2)]],
    uint id [[thread_position_in_grid]]) {
    if (id >= p.count) return;
    float value = a[id];
    switch (p.op) {
        case 0: out[id] = -value; break;
        case 1: out[id] = abs(value); break;
        case 2: out[id] = exp(value); break;
        case 3: out[id] = log(value); break;
        case 4: out[id] = sqrt(value); break;
        default: out[id] = 0.0f; break;
    }
}

kernel void jet_matmul(
    device const float* a [[buffer(0)]],
    device const float* b [[buffer(1)]],
    device float* out [[buffer(2)]],
    constant JetParams& p [[buffer(3)]],
    uint id [[thread_position_in_grid]]) {
    uint total = p.rows * p.cols;
    if (id >= total) return;
    uint row = id / p.cols;
    uint col = id % p.cols;
    float sum = 0.0f;
    for (uint inner = 0; inner < p.inner; inner++) {
        sum = sum + a[row * p.inner + inner] * b[inner * p.cols + col];
    }
    out[id] = sum;
}

kernel void jet_sum(
    device const float* a [[buffer(0)]],
    device float* out [[buffer(1)]],
    constant JetParams& p [[buffer(2)]],
    uint id [[thread_position_in_grid]]) {
    if (id != 0) return;
    float sum = 0.0f;
    for (uint index = 0; index < p.count; index++) sum = sum + a[index];
    out[0] = sum;
}

kernel void jet_mse(
    device const float* a [[buffer(0)]],
    device const float* b [[buffer(1)]],
    device float* out [[buffer(2)]],
    constant JetParams& p [[buffer(3)]],
    uint id [[thread_position_in_grid]]) {
    if (id != 0) return;
    float sum = 0.0f;
    for (uint index = 0; index < p.count; index++) {
        float difference = a[index] - b[index];
        sum = sum + difference * difference;
    }
    out[0] = sum / float(p.count);
}

kernel void jet_mse_grad(
    device const float* a [[buffer(0)]],
    device const float* b [[buffer(1)]],
    device const float* cot [[buffer(2)]],
    device float* out [[buffer(3)]],
    constant JetParams& p [[buffer(4)]],
    uint id [[thread_position_in_grid]]) {
    if (id >= p.count) return;
    float difference = p.op == 0 ? a[id] - b[id] : b[id] - a[id];
    out[id] = difference * (2.0f / float(p.count)) * cot[0];
}

kernel void jet_mse_jvp(
    device const float* a [[buffer(0)]],
    device const float* b [[buffer(1)]],
    device const float* at [[buffer(2)]],
    device const float* bt [[buffer(3)]],
    device float* out [[buffer(4)]],
    constant JetParams& p [[buffer(5)]],
    uint id [[thread_position_in_grid]]) {
    if (id != 0) return;
    float sum = 0.0f;
    for (uint index = 0; index < p.count; index++) {
        sum = sum + 2.0f * (a[index] - b[index]) * (at[index] - bt[index]);
    }
    out[0] = sum / float(p.count);
}

kernel void jet_sgd(
    device const float* parameter [[buffer(0)]],
    device const float* gradient [[buffer(1)]],
    device float* out [[buffer(2)]],
    constant JetParams& p [[buffer(3)]],
    uint id [[thread_position_in_grid]]) {
    if (id >= p.count) return;
    out[id] = parameter[id] - p.scalar * gradient[id];
}

kernel void jet_scale(
    device const float* a [[buffer(0)]],
    device float* out [[buffer(1)]],
    constant JetParams& p [[buffer(2)]],
    uint id [[thread_position_in_grid]]) {
    if (id >= p.count) return;
    out[id] = a[id] * p.scalar;
}

kernel void jet_copy(
    device const float* a [[buffer(0)]],
    device float* out [[buffer(1)]],
    constant JetParams& p [[buffer(2)]],
    uint id [[thread_position_in_grid]]) {
    if (id >= p.count) return;
    out[id] = a[id];
}
"#;

// JET_VETTED_UNSAFE_BEGIN: jet_compute_metal
#[link(name = "Metal", kind = "framework")]
unsafe extern "C" {
    fn MTLCreateSystemDefaultDevice() -> Obj;
}

#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const i8) -> Obj;
    fn sel_registerName(name: *const i8) -> Obj;
    fn objc_msgSend(receiver: Obj, selector: Obj, ...) -> Obj;
}

struct Object(Obj);

impl Object {
    fn new(value: Obj, label: &str) -> Result<Self, JetComputeError> {
        if value == 0 {
            return Err(JetComputeError::Device(format!(
                "Metal {label} allocation or launch failed"
            )));
        }
        Ok(Self(value))
    }

    fn raw(&self) -> Obj {
        self.0
    }
}

impl Drop for Object {
    fn drop(&mut self) {
        unsafe {
            msg0(self.0, b"release\0");
        }
    }
}

unsafe fn selector(name: &[u8]) -> Obj {
    unsafe { sel_registerName(name.as_ptr().cast()) }
}

unsafe fn msg0(receiver: Obj, name: &[u8]) -> Obj {
    unsafe { objc_msgSend(receiver, selector(name)) }
}

unsafe fn msg1(receiver: Obj, name: &[u8], first: Obj) -> Obj {
    unsafe { objc_msgSend(receiver, selector(name), first) }
}

unsafe fn msg2(receiver: Obj, name: &[u8], first: Obj, second: Obj) -> Obj {
    unsafe { objc_msgSend(receiver, selector(name), first, second) }
}

unsafe fn msg3(receiver: Obj, name: &[u8], first: Obj, second: Obj, third: Obj) -> Obj {
    unsafe { objc_msgSend(receiver, selector(name), first, second, third) }
}

unsafe fn msg_size2(receiver: Obj, name: &[u8], first: Size, second: Size) -> Obj {
    unsafe { objc_msgSend(receiver, selector(name), first, second) }
}

fn string(value: &str) -> Result<Obj, JetComputeError> {
    let value = CString::new(value).map_err(|_| {
        JetComputeError::Unsupported("Metal source name contains a NUL byte".to_string())
    })?;
    unsafe {
        let class = objc_getClass(b"NSString\0".as_ptr().cast());
        if class == 0 {
            return Err(JetComputeError::Device(
                "Metal Objective-C NSString class is unavailable".to_string(),
            ));
        }
        let result = msg1(class, b"stringWithUTF8String:\0", value.as_ptr() as Obj);
        if result == 0 {
            return Err(JetComputeError::Device(
                "Metal could not create an Objective-C string".to_string(),
            ));
        }
        Ok(result)
    }
}

fn device() -> Result<Object, JetComputeError> {
    unsafe { Object::new(MTLCreateSystemDefaultDevice(), "device") }
}

pub fn available() -> bool {
    let Ok(device) = device() else {
        return false;
    };
    drop(device);
    true
}

fn object(value: Obj, label: &str) -> Result<Object, JetComputeError> {
    Object::new(value, label)
}

fn u32_value(value: usize, label: &str) -> Result<u32, JetComputeError> {
    u32::try_from(value).map_err(|_| {
        JetComputeError::InvalidShape(format!("Metal {label} exceeds the u32 kernel limit"))
    })
}

fn run(
    function_name: &str,
    inputs: &[&[f32]],
    output_len: usize,
    params: Params,
    output_index: usize,
    params_index: usize,
) -> Result<Vec<f32>, JetComputeError> {
    if output_len == 0 {
        return Ok(Vec::new());
    }
    let device = device()?;
    let source = string(SHADER)?;
    let function_name = string(function_name)?;
    let options_class = unsafe { objc_getClass(b"MTLCompileOptions\0".as_ptr().cast()) };
    if options_class == 0 {
        return Err(JetComputeError::Device(
            "Metal compile options are unavailable".to_string(),
        ));
    }
    let options = unsafe {
        let allocated = msg0(options_class, b"alloc\0");
        let initialized = msg0(allocated, b"init\0");
        let options = object(initialized, "compile options")?;
        msg1(options.raw(), b"setFastMathEnabled:\0", 0);
        options
    };
    let mut compile_error = 0;
    let library = unsafe {
        object(
            msg3(
                device.raw(),
                b"newLibraryWithSource:options:error:\0",
                source,
                options.raw(),
                (&mut compile_error as *mut Obj) as Obj,
            ),
            "library",
        )
        .map_err(|_| {
            JetComputeError::Unsupported(
                "Metal shader compilation rejected the requested kernel".to_string(),
            )
        })?
    };
    let function = unsafe {
        object(
            msg1(
                library.raw(),
                b"newFunctionWithName:\0",
                function_name,
            ),
            "kernel function",
        )
        .map_err(|_| {
            JetComputeError::Unsupported(
                "Metal shader does not contain the requested kernel".to_string(),
            )
        })?
    };
    let mut pipeline_error = 0;
    let pipeline = unsafe {
        object(
            msg2(
                device.raw(),
                b"newComputePipelineStateWithFunction:error:\0",
                function.raw(),
                (&mut pipeline_error as *mut Obj) as Obj,
            ),
            "compute pipeline",
        )
        .map_err(|_| {
            JetComputeError::Unsupported(
                "Metal rejected the requested kernel pipeline".to_string(),
            )
        })?
    };
    let queue = unsafe {
        object(msg0(device.raw(), b"newCommandQueue\0"), "command queue")?
    };
    let command = unsafe {
        object(msg0(queue.raw(), b"commandBuffer\0"), "command buffer")?
    };
    let encoder = unsafe {
        object(
            msg0(command.raw(), b"computeCommandEncoder\0"),
            "compute encoder",
        )?
    };
    let mut input_buffers = Vec::with_capacity(inputs.len());
    for values in inputs {
        let bytes = values
            .len()
            .checked_mul(std::mem::size_of::<f32>())
            .ok_or_else(|| JetComputeError::InvalidShape("Metal buffer size overflow".to_string()))?;
        let buffer = unsafe {
            object(
                msg3(
                    device.raw(),
                    b"newBufferWithBytes:length:options:\0",
                    values.as_ptr() as Obj,
                    bytes,
                    0,
                ),
                "input buffer",
            )?
        };
        input_buffers.push(buffer);
    }
    let output_bytes = output_len
        .checked_mul(std::mem::size_of::<f32>())
        .ok_or_else(|| JetComputeError::InvalidShape("Metal output buffer size overflow".to_string()))?;
    let output = unsafe {
        object(
            msg2(
                device.raw(),
                b"newBufferWithLength:options:\0",
                output_bytes,
                0,
            ),
            "output buffer",
        )?
    };
    unsafe {
        msg1(
            encoder.raw(),
            b"setComputePipelineState:\0",
            pipeline.raw(),
        );
        for (index, buffer) in input_buffers.iter().enumerate() {
            msg3(
                encoder.raw(),
                b"setBuffer:offset:atIndex:\0",
                buffer.raw(),
                0,
                index,
            );
        }
        msg3(
            encoder.raw(),
            b"setBuffer:offset:atIndex:\0",
            output.raw(),
            0,
            output_index,
        );
        msg3(
            encoder.raw(),
            b"setBytes:length:atIndex:\0",
            (&params as *const Params) as Obj,
            std::mem::size_of::<Params>(),
            params_index,
        );
        msg_size2(
            encoder.raw(),
            b"dispatchThreads:threadsPerThreadgroup:\0",
            Size {
                width: output_len,
                height: 1,
                depth: 1,
            },
            Size {
                width: 1,
                height: 1,
                depth: 1,
            },
        );
        msg0(encoder.raw(), b"endEncoding\0");
        msg0(command.raw(), b"commit\0");
        msg0(command.raw(), b"waitUntilCompleted\0");
        if msg0(command.raw(), b"status\0") != STATUS_COMPLETED {
            return Err(JetComputeError::Device(
                "Metal command buffer failed or device was lost".to_string(),
            ));
        }
        let contents = msg0(output.raw(), b"contents\0");
        if contents == 0 {
            return Err(JetComputeError::Device(
                "Metal output buffer has no CPU-readable contents".to_string(),
            ));
        }
        let values = std::slice::from_raw_parts(contents as *const f32, output_len).to_vec();
        if values.iter().any(|value| !value.is_finite()) {
            return Err(JetComputeError::Arithmetic(
                "Metal kernel produced a non-finite F32 value".to_string(),
            ));
        }
        Ok(values)
    }
}

pub fn copy(values: &[f32]) -> Result<Vec<f32>, JetComputeError> {
    let count = u32_value(values.len(), "copy count")?;
    run(
        "jet_copy",
        &[values],
        values.len(),
        Params {
            count,
            rows: 0,
            inner: 0,
            cols: 0,
            op: 0,
            scalar: 0.0,
        },
        1,
        2,
    )
}

pub fn binary(op: u32, left: &[f32], right: &[f32]) -> Result<Vec<f32>, JetComputeError> {
    if left.len() != right.len() {
        return Err(JetComputeError::InvalidShape(
            "Metal binary inputs have different lengths".to_string(),
        ));
    }
    let count = u32_value(left.len(), "binary count")?;
    run(
        "jet_binary",
        &[left, right],
        left.len(),
        Params {
            count,
            rows: 0,
            inner: 0,
            cols: 0,
            op,
            scalar: 0.0,
        },
        2,
        3,
    )
}

pub fn unary(op: u32, values: &[f32]) -> Result<Vec<f32>, JetComputeError> {
    let count = u32_value(values.len(), "unary count")?;
    run(
        "jet_unary",
        &[values],
        values.len(),
        Params {
            count,
            rows: 0,
            inner: 0,
            cols: 0,
            op,
            scalar: 0.0,
        },
        1,
        2,
    )
}

pub fn matmul(
    left: &[f32],
    right: &[f32],
    rows: usize,
    inner: usize,
    cols: usize,
) -> Result<Vec<f32>, JetComputeError> {
    let count = rows
        .checked_mul(cols)
        .ok_or_else(|| JetComputeError::InvalidShape("Metal matmul output size overflow".to_string()))?;
    let params = Params {
        count: u32_value(count, "matmul output")?,
        rows: u32_value(rows, "matmul rows")?,
        inner: u32_value(inner, "matmul inner dimension")?,
        cols: u32_value(cols, "matmul columns")?,
        op: 0,
        scalar: 0.0,
    };
    run("jet_matmul", &[left, right], count, params, 2, 3)
}

pub fn sum(values: &[f32]) -> Result<Vec<f32>, JetComputeError> {
    let count = u32_value(values.len(), "sum count")?;
    run(
        "jet_sum",
        &[values],
        1,
        Params {
            count,
            rows: 0,
            inner: 0,
            cols: 0,
            op: 0,
            scalar: 0.0,
        },
        1,
        2,
    )
}

pub fn mse(left: &[f32], right: &[f32]) -> Result<Vec<f32>, JetComputeError> {
    if left.len() != right.len() || left.is_empty() {
        return Err(JetComputeError::InvalidShape(
            "Metal MSE inputs must have the same non-empty length".to_string(),
        ));
    }
    run(
        "jet_mse",
        &[left, right],
        1,
        Params {
            count: u32_value(left.len(), "MSE count")?,
            rows: 0,
            inner: 0,
            cols: 0,
            op: 0,
            scalar: 0.0,
        },
        2,
        3,
    )
}

pub fn mse_grad(
    left: &[f32],
    right: &[f32],
    cot: &[f32],
    positive: bool,
) -> Result<Vec<f32>, JetComputeError> {
    if left.len() != right.len() || left.is_empty() || cot.len() != 1 {
        return Err(JetComputeError::InvalidShape(
            "Metal MSE gradient inputs have incompatible lengths".to_string(),
        ));
    }
    run(
        "jet_mse_grad",
        &[left, right, cot],
        left.len(),
        Params {
            count: u32_value(left.len(), "MSE gradient count")?,
            rows: 0,
            inner: 0,
            cols: 0,
            op: u32::from(!positive),
            scalar: 0.0,
        },
        3,
        4,
    )
}

pub fn mse_jvp(
    left: &[f32],
    right: &[f32],
    left_tangent: &[f32],
    right_tangent: &[f32],
) -> Result<Vec<f32>, JetComputeError> {
    if left.len() != right.len()
        || left.len() != left_tangent.len()
        || left.len() != right_tangent.len()
        || left.is_empty()
    {
        return Err(JetComputeError::InvalidShape(
            "Metal MSE JVP inputs have incompatible lengths".to_string(),
        ));
    }
    run(
        "jet_mse_jvp",
        &[left, right, left_tangent, right_tangent],
        1,
        Params {
            count: u32_value(left.len(), "MSE JVP count")?,
            rows: 0,
            inner: 0,
            cols: 0,
            op: 0,
            scalar: 0.0,
        },
        4,
        5,
    )
}

pub fn sgd(
    parameter: &[f32],
    gradient: &[f32],
    learning_rate: f32,
) -> Result<Vec<f32>, JetComputeError> {
    if parameter.len() != gradient.len() {
        return Err(JetComputeError::InvalidShape(
            "Metal SGD inputs have different lengths".to_string(),
        ));
    }
    run(
        "jet_sgd",
        &[parameter, gradient],
        parameter.len(),
        Params {
            count: u32_value(parameter.len(), "SGD count")?,
            rows: 0,
            inner: 0,
            cols: 0,
            op: 0,
            scalar: learning_rate,
        },
        2,
        3,
    )
}

pub fn scale(values: &[f32], scalar: f32) -> Result<Vec<f32>, JetComputeError> {
    run(
        "jet_scale",
        &[values],
        values.len(),
        Params {
            count: u32_value(values.len(), "scale count")?,
            rows: 0,
            inner: 0,
            cols: 0,
            op: 0,
            scalar,
        },
        1,
        2,
    )
}
}
// JET_VETTED_UNSAFE_END: jet_compute_metal

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
mod jet_compute_metal {
    use super::JetComputeError;

    fn unavailable<T>() -> Result<T, JetComputeError> {
        Err(JetComputeError::Unsupported(
            "Metal backend is unavailable on this target".to_string(),
        ))
    }

    pub fn available() -> bool {
        false
    }

    pub fn copy(_: &[f32]) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn binary(_: u32, _: &[f32], _: &[f32]) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn unary(_: u32, _: &[f32]) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn matmul(_: &[f32], _: &[f32], _: usize, _: usize, _: usize) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn sum(_: &[f32]) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn mse(_: &[f32], _: &[f32]) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn mse_grad(_: &[f32], _: &[f32], _: &[f32], _: bool) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn mse_jvp(_: &[f32], _: &[f32], _: &[f32], _: &[f32]) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn sgd(_: &[f32], _: &[f32], _: f32) -> Result<Vec<f32>, JetComputeError> { unavailable() }
    pub fn scale(_: &[f32], _: f32) -> Result<Vec<f32>, JetComputeError> { unavailable() }
}

fn jet_compute_registered_abilities(profile: &str) -> Option<&'static [&'static str]> {
    match profile {
        CPU_ORACLE_F64_PROFILE => Some(CPU_ORACLE_F64_CAPABILITIES),
        CPU_ORACLE_F32_PROFILE => Some(CPU_ORACLE_F32_CAPABILITIES),
        _ => None,
    }
}

fn jet_compute_registered_backend_abilities(
    backend: &str,
    profile: &str,
) -> Option<&'static [&'static str]> {
    match backend {
        CPU_ORACLE_BACKEND => jet_compute_registered_abilities(profile),
        METAL_BACKEND if profile == CPU_ORACLE_F32_PROFILE => Some(METAL_F32_CAPABILITIES),
        _ => None,
    }
}

fn jet_compute_abilities_match(actual: &[String], expected: &[&str]) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected.iter())
            .all(|(actual, expected)| actual == expected)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetComputePlacementReceipt {
    requested: JetComputeDevice,
    selected: JetComputeDevice,
    backend: String,
    version: String,
    profile: String,
    cache: String,
    abilities: Vec<String>,
    reason: String,
}

#[derive(Clone)]
struct JetComputeTrace {
    // Traces are observations of a live per-call tape.  The transform state
    // owns the strong tape handle; graph values keep only a weak back-link so
    // nested tapes cannot retain one another through recorded values.
    tape: std::sync::Weak<std::sync::Mutex<JetComputeTape>>,
    node: usize,
    parent: Option<Box<JetComputeTrace>>,
}

impl std::fmt::Debug for JetComputeTrace {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JetComputeTrace")
            .field("node", &self.node)
            .field("parent", &self.parent)
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct JetTensor {
    shape: Vec<i64>,
    strides: Vec<i64>,
    data: std::sync::Arc<Vec<f64>>,
    device: JetComputeDevice,
    last_placement: JetComputePlacementReceipt,
    last_transfer: Option<JetComputeTransferReceipt>,
    trace: Option<JetComputeTrace>,
}

/// A compiler-internal mutable Tensor window. Unlike an ordinary list view it
/// retains the owner and original range so every element write can use the
/// complete shared window policy.
pub struct JetComputeViewMut<'a> {
    tensor: &'a mut JetTensor,
    start: i64,
    end: i64,
    exclusive: bool,
}

impl PartialEq for JetTensor {
    fn eq(&self, other: &Self) -> bool {
        self.shape == other.shape
            && self.strides == other.strides
            && self.data == other.data
            && self.device == other.device
            && self.last_placement == other.last_placement
            && self.last_transfer == other.last_transfer
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum JetComputeError {
    InvalidShape(String),
    RankMismatch(String),
    OutOfBounds(String),
    Device(String),
    Unsupported(String),
    Arithmetic(String),
    Serialization(String),
}

impl JetShow for JetComputeError {
    fn jet_show(&self) -> String {
        match self {
            JetComputeError::InvalidShape(m)
            | JetComputeError::RankMismatch(m)
            | JetComputeError::OutOfBounds(m)
            | JetComputeError::Device(m)
            | JetComputeError::Unsupported(m)
            | JetComputeError::Arithmetic(m)
            | JetComputeError::Serialization(m) => m.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum JetComputeTapeRule {
    Add,
    Sub,
    Mul,
    Div,
    Maximum,
    Minimum,
    Matmul,
    MatmulF32Tile,
    MseLoss,
    SgdStep {
        learning_rate: f64,
    },
    Unary(String),
    Reshape {
        source_shape: Vec<i64>,
    },
    Broadcast {
        source_shape: Vec<i64>,
    },
    ReduceToShape {
        source_shape: Vec<i64>,
    },
    Transpose,
    SumAxis {
        axis: usize,
        source_shape: Vec<i64>,
    },
}

#[derive(Clone, Debug, PartialEq)]
struct JetComputeTapeNode {
    parents: Vec<Option<usize>>,
    rule: Option<JetComputeTapeRule>,
    values: Vec<JetTensor>,
    output: JetTensor,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetComputeTape {
    nodes: Vec<JetComputeTapeNode>,
    inputs: Vec<JetTensor>,
}

#[derive(Clone, Debug)]
pub struct JetComputeVjpState {
    value: JetTensor,
    tape: std::sync::Arc<std::sync::Mutex<JetComputeTape>>,
    output_node: Option<usize>,
}

enum JetComputeTransformResult {
    Gradient(Vec<JetTensor>),
    ValueAndGradient {
        value: JetTensor,
        gradients: Vec<JetTensor>,
    },
    Vjp {
        value: JetTensor,
        state: JetComputeVjpState,
    },
    Jvp {
        value: JetTensor,
        tangent: JetTensor,
    },
}

/// One transform meaning crosses every host boundary.  The numeric form is
/// also the resident host ABI; hosts marshal it but never select policy from
/// a method string.
#[repr(i64)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetComputeTransformKind {
    Gradient = 0,
    ValueAndGradient = 1,
    Vjp = 2,
    Jvp = 3,
}

impl JetComputeTransformKind {
    pub fn from_i64(value: i64) -> Option<Self> {
        match value {
            0 => Some(Self::Gradient),
            1 => Some(Self::ValueAndGradient),
            2 => Some(Self::Vjp),
            3 => Some(Self::Jvp),
            _ => None,
        }
    }

    pub fn is_jvp(self) -> bool {
        matches!(self, Self::Jvp)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Gradient => "gradient",
            Self::ValueAndGradient => "value_and_gradient",
            Self::Vjp => "vjp",
            Self::Jvp => "jvp",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetComputeResultShape {
    Tensor,
    TensorTuple(usize),
}

pub enum JetComputeBaseResult {
    Tensor(JetTensor),
    TensorTuple(Vec<JetTensor>),
}

pub struct JetComputeBase {
    arity: usize,
    invoke: std::rc::Rc<dyn Fn(&[JetTensor]) -> Result<JetComputeBaseResult, JetComputeError>>,
}

impl Clone for JetComputeBase {
    fn clone(&self) -> Self {
        Self {
            arity: self.arity,
            invoke: self.invoke.clone(),
        }
    }
}

impl JetComputeBase {
    pub fn new<F>(arity: usize, invoke: F) -> Self
    where
        F: Fn(&[JetTensor]) -> Result<JetComputeBaseResult, JetComputeError> + 'static,
    {
        Self {
            arity,
            invoke: std::rc::Rc::new(invoke),
        }
    }

    fn call(&self, inputs: &[JetTensor]) -> Result<JetComputeBaseResult, JetComputeError> {
        if inputs.len() != self.arity {
            return Err(JetComputeError::Unsupported(
                "autodiff callable received the wrong number of Tensor arguments".to_string(),
            ));
        }
        (self.invoke)(inputs)
    }
}

pub struct JetComputeInputPack {
    pub primals: Vec<JetTensor>,
    pub tangents: Vec<JetTensor>,
    flat: bool,
}

impl JetComputeInputPack {
    pub fn new(primals: Vec<JetTensor>, tangents: Vec<JetTensor>) -> Self {
        Self {
            primals,
            tangents,
            flat: false,
        }
    }

    /// Resident hosts receive one flat list from a typed function-value ABI.
    /// The plan owns the JVP split, so the host does not inspect transform
    /// policy while marshalling that list.
    pub fn from_flat(values: Vec<JetTensor>) -> Self {
        Self {
            primals: values,
            tangents: Vec::new(),
            flat: true,
        }
    }
}

pub enum JetComputeCurriedResult {
    Gradient(Vec<Vec<JetTensor>>),
    ValueAndGradient {
        value: JetTensor,
        gradients: Vec<Vec<JetTensor>>,
    },
    Vjp {
        value: JetTensor,
        pull: i64,
        grads: i64,
    },
    Jvp {
        value: JetTensor,
        tangent: JetTensor,
    },
}

#[derive(Clone, Copy)]
enum JetComputeCurriedContinuation {
    Pull,
    Grads,
}

#[derive(Clone)]
enum JetComputeCurriedEntry {
    Plan {
        base: JetComputeBase,
        kind: JetComputeTransformKind,
        targets: Vec<i64>,
        result_shape: JetComputeResultShape,
    },
    Continuation {
        state: JetComputeVjpState,
        targets: Vec<i64>,
        kind: JetComputeCurriedContinuation,
    },
}

struct JetComputeCurriedSlot {
    refs: usize,
    entry: JetComputeCurriedEntry,
}

thread_local! {
    static JET_COMPUTE_CURRIED_HANDLES:
        std::cell::RefCell<Vec<Option<JetComputeCurriedSlot>>> = const {
            std::cell::RefCell::new(Vec::new())
        };
}

fn jet_compute_curried_insert(entry: JetComputeCurriedEntry) -> i64 {
    JET_COMPUTE_CURRIED_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        let index = handles.len();
        handles.push(Some(JetComputeCurriedSlot { refs: 1, entry }));
        (index as i64).saturating_add(1)
    })
}

fn jet_compute_curried_entry(handle: i64) -> Option<JetComputeCurriedEntry> {
    let index = usize::try_from(handle).ok()?.checked_sub(1)?;
    JET_COMPUTE_CURRIED_HANDLES.with(|handles| {
        handles
            .borrow()
            .get(index)
            .and_then(Option::as_ref)
            .map(|slot| slot.entry.clone())
    })
}

pub fn jet_compute_curried_new(
    base: JetComputeBase,
    kind: JetComputeTransformKind,
    targets: &[i64],
    result_shape: JetComputeResultShape,
) -> i64 {
    jet_compute_curried_insert(JetComputeCurriedEntry::Plan {
        base,
        kind,
        targets: targets.to_vec(),
        result_shape,
    })
}

pub fn jet_compute_curried_clone(handle: i64) -> i64 {
    let Some(index) = usize::try_from(handle).ok().and_then(|value| value.checked_sub(1)) else {
        return 0;
    };
    JET_COMPUTE_CURRIED_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        let Some(slot) = handles.get_mut(index).and_then(Option::as_mut) else {
            return 0;
        };
        slot.refs = slot.refs.saturating_add(1);
        handle
    })
}

pub fn jet_compute_curried_drop(handle: i64) {
    let Some(index) = usize::try_from(handle).ok().and_then(|value| value.checked_sub(1)) else {
        return;
    };
    JET_COMPUTE_CURRIED_HANDLES.with(|handles| {
        let mut handles = handles.borrow_mut();
        let remove = match handles.get_mut(index).and_then(Option::as_mut) {
            Some(slot) if slot.refs <= 1 => true,
            Some(slot) => {
                slot.refs -= 1;
                false
            }
            None => return,
        };
        if remove {
            handles[index] = None;
        }
    });
}

#[repr(transparent)]
pub struct JetComputeHandle(i64);

impl JetComputeHandle {
    pub fn new(raw: i64) -> Self {
        Self(raw)
    }

    pub fn raw(&self) -> i64 {
        self.0
    }
}

impl Clone for JetComputeHandle {
    fn clone(&self) -> Self {
        Self(jet_compute_curried_clone(self.0))
    }
}

impl Drop for JetComputeHandle {
    fn drop(&mut self) {
        jet_compute_curried_drop(self.0);
    }
}

fn jet_compute_curried_result_shape(
    result: JetComputeBaseResult,
    shape: JetComputeResultShape,
) -> Result<Vec<JetTensor>, JetComputeError> {
    let values = match (shape, result) {
        (JetComputeResultShape::Tensor, JetComputeBaseResult::Tensor(value)) => {
            vec![value]
        }
        (JetComputeResultShape::TensorTuple(expected), JetComputeBaseResult::TensorTuple(values)) => {
            if values.len() != expected {
                return Err(JetComputeError::Unsupported(format!(
                    "autodiff base returned {} tensors; expected {expected}",
                    values.len()
                )));
            }
            values
        }
        (JetComputeResultShape::Tensor, JetComputeBaseResult::TensorTuple(_))
        | (JetComputeResultShape::TensorTuple(_), JetComputeBaseResult::Tensor(_)) => {
            return Err(JetComputeError::Unsupported(
                "autodiff base returned the wrong result shape".to_string(),
            ));
        }
    };
    for value in &values {
        jet_compute_validate_tensor(value)?;
    }
    Ok(values)
}

fn jet_compute_curried_gradient_result(
    states: &[JetComputeVjpState],
    targets: &[i64],
) -> Result<Vec<Vec<JetTensor>>, JetComputeError> {
    let gradients = states
        .iter()
        .map(|state| {
            let seed = jet_compute_gradient_seed(state)?;
            jet_compute_vjp_pull(state, &seed, targets)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if gradients.len() == 1 {
        return Ok(gradients
            .into_iter()
            .next()
            .unwrap_or_default()
            .into_iter()
            .map(|gradient| vec![gradient])
            .collect());
    }
    let mut result = Vec::with_capacity(targets.len());
    for target_index in 0..targets.len() {
        result.push(
            gradients
                .iter()
                .map(|values| values[target_index].clone())
                .collect(),
        );
    }
    Ok(result)
}

fn jet_compute_curried_call_plan(
    base: &JetComputeBase,
    kind: JetComputeTransformKind,
    targets: &[i64],
    result_shape: JetComputeResultShape,
    input: JetComputeInputPack,
) -> Result<JetComputeCurriedResult, JetComputeError> {
    for tensor in input.primals.iter().chain(input.tangents.iter()) {
        jet_compute_validate_tensor(tensor)?;
    }
    if matches!(result_shape, JetComputeResultShape::TensorTuple(_))
        && !matches!(kind, JetComputeTransformKind::Gradient)
    {
        return Err(JetComputeError::Unsupported(
            "only compute.gradient can transform a Tensor tuple".to_string(),
        ));
    }
    let (tape, tracked) = jet_compute_trace_inputs(input.primals);
    let result = base.call(&tracked)?;
    let values = jet_compute_curried_result_shape(result, result_shape)?;
    let states = values
        .iter()
        .cloned()
        .map(|value| jet_compute_vjp_begin(value, tape.clone()))
        .collect::<Vec<_>>();
    match kind {
        JetComputeTransformKind::Gradient => Ok(JetComputeCurriedResult::Gradient(
            jet_compute_curried_gradient_result(&states, targets)?,
        )),
        JetComputeTransformKind::ValueAndGradient => {
            let Some(state) = states.first() else {
                return Err(JetComputeError::Unsupported(
                    "value_and_gradient base returned no value".to_string(),
                ));
            };
            Ok(JetComputeCurriedResult::ValueAndGradient {
                value: jet_compute_remove_trace_level(&state.value, &state.tape),
                gradients: jet_compute_curried_gradient_result(&states, targets)?,
            })
        }
        JetComputeTransformKind::Vjp => {
            let Some(state) = states.first() else {
                return Err(JetComputeError::Unsupported(
                    "vjp base returned no value".to_string(),
                ));
            };
            let value = jet_compute_remove_trace_level(&state.value, &state.tape);
            let pull = jet_compute_curried_insert(JetComputeCurriedEntry::Continuation {
                state: state.clone(),
                targets: targets.to_vec(),
                kind: JetComputeCurriedContinuation::Pull,
            });
            let grads = jet_compute_curried_insert(JetComputeCurriedEntry::Continuation {
                state: state.clone(),
                targets: targets.to_vec(),
                kind: JetComputeCurriedContinuation::Grads,
            });
            Ok(JetComputeCurriedResult::Vjp { value, pull, grads })
        }
        JetComputeTransformKind::Jvp => {
            let Some(state) = states.first() else {
                return Err(JetComputeError::Unsupported(
                    "jvp base returned no value".to_string(),
                ));
            };
            Ok(JetComputeCurriedResult::Jvp {
                value: jet_compute_remove_trace_level(&state.value, &state.tape),
                tangent: jet_compute_jvp(state, input.tangents)?,
            })
        }
    }
}

fn jet_compute_curried_flat_input(
    entry: &JetComputeCurriedEntry,
    input: JetComputeInputPack,
) -> Result<JetComputeInputPack, JetComputeError> {
    if !input.flat {
        return Ok(input);
    }
    let JetComputeInputPack { primals, .. } = input;
    let JetComputeCurriedEntry::Plan { base, kind, .. } = entry else {
        return Ok(JetComputeInputPack::new(primals, Vec::new()));
    };
    if !kind.is_jvp() {
        return Ok(JetComputeInputPack::new(primals, Vec::new()));
    }
    let split = base.arity;
    if primals.len() != split.saturating_mul(2) {
        return Err(JetComputeError::Unsupported(
            "jvp needs one tangent for every primal".to_string(),
        ));
    }
    Ok(JetComputeInputPack::new(
        primals[..split].to_vec(),
        primals[split..].to_vec(),
    ))
}

pub fn jet_compute_call_curried(
    handle: i64,
    input: JetComputeInputPack,
) -> Result<JetComputeCurriedResult, JetComputeError> {
    let Some(entry) = jet_compute_curried_entry(handle) else {
        return Err(JetComputeError::Unsupported(
            "autodiff callable handle is invalid or expired".to_string(),
        ));
    };
    let input = jet_compute_curried_flat_input(&entry, input)?;
    match entry {
        JetComputeCurriedEntry::Plan {
            base,
            kind,
            targets,
            result_shape,
        } => {
            if kind.is_jvp() {
                if input.primals.len() != input.tangents.len() {
                    return Err(JetComputeError::Unsupported(
                        "jvp needs one tangent for every primal".to_string(),
                    ));
                }
            } else if !input.tangents.is_empty() {
                return Err(JetComputeError::Unsupported(
                    "non-JVP autodiff callable received tangent values".to_string(),
                ));
            }
            jet_compute_curried_call_plan(&base, kind, &targets, result_shape, input)
        }
        JetComputeCurriedEntry::Continuation {
            state,
            targets,
            kind,
        } => {
            if !input.tangents.is_empty() || input.primals.len() != usize::from(matches!(kind, JetComputeCurriedContinuation::Pull)) {
                return Err(JetComputeError::Unsupported(
                    "autodiff continuation received the wrong arguments".to_string(),
                ));
            }
            let gradients = match kind {
                JetComputeCurriedContinuation::Pull => {
                    jet_compute_vjp_pull(&state, &input.primals[0], &targets)?
                }
                JetComputeCurriedContinuation::Grads => {
                    let seed = jet_compute_gradient_seed(&state)?;
                    jet_compute_vjp_pull(&state, &seed, &targets)?
                }
            };
            Ok(JetComputeCurriedResult::Gradient(
                gradients.into_iter().map(|gradient| vec![gradient]).collect(),
            ))
        }
    }
}

pub fn jet_compute_call_curried_or_panic(
    handle: i64,
    input: JetComputeInputPack,
    context: &str,
) -> JetComputeCurriedResult {
    match jet_compute_call_curried(handle, input) {
        Ok(result) => result,
        Err(error) => jet_panic("Compute.rs", line!(), &format!("{context}: {}", error.jet_show())),
    }
}

struct JetComputeVjpRun<R> {
    pub value: JetTensor,
    pub pull: std::rc::Rc<dyn Fn(&JetTensor) -> R>,
    pub grads: std::rc::Rc<dyn Fn() -> R>,
}

impl<R: Clone> Clone for JetComputeVjpRun<R> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            pull: self.pull.clone(),
            grads: self.grads.clone(),
        }
    }
}

impl<R> JetComputeVjpRun<R> {
    fn grads_or_panic(&self) -> R {
        (self.grads)()
    }
}

fn jet_compute_untracked(tensor: &JetTensor) -> JetTensor {
    let mut value = tensor.clone();
    value.trace = None;
    value
}

fn jet_compute_trace_node_for_tape(
    trace: Option<&JetComputeTrace>,
    tape: &std::sync::Arc<std::sync::Mutex<JetComputeTape>>,
) -> Option<usize> {
    let trace = trace?;
    if std::sync::Weak::ptr_eq(&trace.tape, &std::sync::Arc::downgrade(tape)) {
        return Some(trace.node);
    }
    jet_compute_trace_node_for_tape(trace.parent.as_deref(), tape)
}

fn jet_compute_trace_lanes(
    trace: Option<&JetComputeTrace>,
) -> Vec<(
    std::sync::Arc<std::sync::Mutex<JetComputeTape>>,
    usize,
)> {
    let mut lanes = Vec::new();
    let mut current = trace;
    while let Some(trace) = current {
        if let Some(tape) = trace.tape.upgrade() {
            lanes.push((tape, trace.node));
        }
        current = trace.parent.as_deref();
    }
    lanes
}

fn jet_compute_remove_trace_level(
    tensor: &JetTensor,
    tape: &std::sync::Arc<std::sync::Mutex<JetComputeTape>>,
) -> JetTensor {
    fn remove(
        trace: JetComputeTrace,
        tape: &std::sync::Arc<std::sync::Mutex<JetComputeTape>>,
    ) -> (Option<JetComputeTrace>, bool) {
        if std::sync::Weak::ptr_eq(&trace.tape, &std::sync::Arc::downgrade(tape)) {
            return (trace.parent.map(|parent| *parent), true);
        }
        let (parent, removed) = match trace.parent {
            Some(parent) => {
                let (parent, removed) = remove(*parent, tape);
                (parent.map(Box::new), removed)
            }
            None => (None, false),
        };
        (
            Some(JetComputeTrace {
                tape: trace.tape,
                node: trace.node,
                parent,
            }),
            removed,
        )
    }

    let Some(trace) = tensor.trace.clone() else {
        return tensor.clone();
    };
    let (trace, _) = remove(trace, tape);
    let mut value = tensor.clone();
    value.trace = trace.and_then(|trace| jet_compute_prune_trace(Some(trace)));
    value
}

fn jet_compute_prune_trace(trace: Option<JetComputeTrace>) -> Option<JetComputeTrace> {
    let trace = trace?;
    let parent = trace
        .parent
        .and_then(|parent| jet_compute_prune_trace(Some(*parent)).map(Box::new));
    if trace.tape.upgrade().is_none() {
        return parent.map(|parent| *parent);
    }
    Some(JetComputeTrace {
        tape: trace.tape,
        node: trace.node,
        parent,
    })
}

fn jet_compute_tape_for_parents(
    parents: &[&JetTensor],
) -> Result<
    Vec<(
        std::sync::Arc<std::sync::Mutex<JetComputeTape>>,
        Vec<Option<usize>>,
    )>,
    JetComputeError,
> {
    let nonempty = parents
        .iter()
        .map(|parent| jet_compute_trace_lanes(parent.trace.as_ref()))
        .filter(|lanes| !lanes.is_empty())
        .collect::<Vec<_>>();
    let Some(first) = nonempty.first() else {
        return Ok(Vec::new());
    };
    if nonempty.iter().skip(1).any(|lanes| {
        !lanes.iter().any(|(tape, _)| {
            first
                .iter()
                .any(|(first_tape, _)| std::sync::Arc::ptr_eq(first_tape, tape))
        })
    }) {
        return Err(JetComputeError::Unsupported(
            "autodiff values belong to different tapes".to_string(),
        ));
    }
    let mut tapes = Vec::new();
    for lanes in &nonempty {
        for (tape, _) in lanes {
            if !tapes
                .iter()
                .any(|existing| std::sync::Arc::ptr_eq(existing, tape))
            {
                tapes.push(tape.clone());
            }
        }
    }
    Ok(tapes
        .into_iter()
        .map(|tape| {
            let ids = parents
                .iter()
                .map(|parent| {
                    jet_compute_trace_node_for_tape(parent.trace.as_ref(), &tape)
                })
                .collect();
            (tape, ids)
        })
        .collect())
}

fn jet_compute_record(
    mut output: JetTensor,
    parents: &[&JetTensor],
    values: Vec<JetTensor>,
    rule: JetComputeTapeRule,
) -> Result<JetTensor, JetComputeError> {
    // Validate before attaching a trace.  A profile is part of Tensor
    // meaning, so a recorded F32 value must be canonical just like an eager
    // value; no engine may hide a precision mismatch in tape metadata.
    jet_compute_validate_tensor(&output)?;
    let tapes = jet_compute_tape_for_parents(parents)?;
    if tapes.is_empty() {
        return Ok(output);
    }
    let mut recorded = Vec::with_capacity(tapes.len());
    for (tape, parent_ids) in tapes {
        let mut tape_guard = tape
            .lock()
            .map_err(|_| JetComputeError::Unsupported("autodiff tape is poisoned".to_string()))?;
        let node = tape_guard.nodes.len();
        tape_guard.nodes.push(JetComputeTapeNode {
            parents: parent_ids,
            rule: Some(rule.clone()),
            values: values
                .iter()
                .map(|value| jet_compute_remove_trace_level(value, &tape))
                .collect(),
            output: jet_compute_remove_trace_level(&output, &tape),
        });
        recorded.push((tape.clone(), node));
    }
    let mut trace = None;
    for (tape, node) in recorded.into_iter().rev() {
        trace = Some(Box::new(JetComputeTrace {
            tape: std::sync::Arc::downgrade(&tape),
            node,
            parent: trace,
        }));
    }
    output.trace = trace.map(|trace| *trace);
    Ok(output)
}

fn jet_compute_trace_inputs(
    inputs: Vec<JetTensor>,
) -> (
    std::sync::Arc<std::sync::Mutex<JetComputeTape>>,
    Vec<JetTensor>,
) {
    let values = inputs.clone();
    let tape = std::sync::Arc::new(std::sync::Mutex::new(JetComputeTape {
        nodes: Vec::new(),
        inputs: values.clone(),
    }));
    let mut tracked = Vec::with_capacity(values.len());
    let mut guard = tape
        .lock()
        .unwrap_or_else(|_| jet_panic("Compute.rs", line!(), "autodiff tape is poisoned"));
    for (index, value) in values.iter().enumerate() {
        let node = guard.nodes.len();
        guard.nodes.push(JetComputeTapeNode {
            parents: Vec::new(),
            rule: None,
            values: vec![value.clone()],
            output: value.clone(),
        });
        let mut input = value.clone();
        input.trace = Some(JetComputeTrace {
            tape: std::sync::Arc::downgrade(&tape),
            node,
            parent: value.trace.clone().map(Box::new),
        });
        tracked.push(input);
        debug_assert_eq!(index, node);
    }
    drop(guard);
    (tape, tracked)
}

fn jet_compute_empty_tape() -> std::sync::Arc<std::sync::Mutex<JetComputeTape>> {
    std::sync::Arc::new(std::sync::Mutex::new(JetComputeTape {
        nodes: Vec::new(),
        inputs: Vec::new(),
    }))
}

fn jet_compute_vjp_begin(
    value: JetTensor,
    tape: std::sync::Arc<std::sync::Mutex<JetComputeTape>>,
) -> JetComputeVjpState {
    JetComputeVjpState {
        output_node: jet_compute_trace_node_for_tape(value.trace.as_ref(), &tape),
        value,
        tape,
    }
}

impl JetShow for JetComputeDevice {
    fn jet_show(&self) -> String {
        match self {
            JetComputeDevice::Auto => "Auto".to_string(),
            JetComputeDevice::Cpu => "CPU".to_string(),
            JetComputeDevice::Metal => "Metal".to_string(),
        }
    }
}

impl JetShow for JetComputePlacementReceipt {
    fn jet_show(&self) -> String {
        format!(
            "Placement(requested={}, selected={}, backend={}, version={}, profile={}, cache={}, abilities={:?}, reason={})",
            self.requested.jet_show(),
            self.selected.jet_show(),
            self.backend,
            self.version,
            self.profile,
            self.cache,
            self.abilities,
            self.reason
        )
    }
}

impl JetShow for JetTensor {
    fn jet_show(&self) -> String {
        format!(
            "Tensor(shape={:?}, device={}, len={})",
            self.shape,
            self.device.jet_show(),
            jet_compute_tensor_numel(self)
        )
    }
}

fn jet_compute_row_major_strides(shape: &[i64]) -> Result<Vec<i64>, JetComputeError> {
    if shape.is_empty() {
        return Err(JetComputeError::InvalidShape(
            "Tensor shape must have at least one axis".to_string(),
        ));
    }
    if shape.iter().any(|d| *d < 0) {
        return Err(JetComputeError::InvalidShape(
            "Tensor shape axes must be non-negative".to_string(),
        ));
    }
    if shape
        .iter()
        .any(|d| *d > i64::try_from(MAX_TENSOR_ELEMENTS).unwrap_or(i64::MAX))
    {
        return Err(JetComputeError::InvalidShape(
            "Tensor shape axis exceeds the Core storage limit".to_string(),
        ));
    }
    let mut strides = vec![1i64; shape.len()];
    for i in (0..shape.len().saturating_sub(1)).rev() {
        let next = shape[i + 1]
            .checked_mul(strides[i + 1])
            .ok_or_else(|| JetComputeError::InvalidShape("Tensor stride overflow".to_string()))?;
        strides[i] = next;
    }
    Ok(strides)
}

fn jet_compute_numel(shape: &[i64]) -> Result<i64, JetComputeError> {
    let mut n = 1i64;
    for &d in shape {
        if d < 0 {
            return Err(JetComputeError::InvalidShape(
                "Tensor shape axes must be non-negative".to_string(),
            ));
        }
        n = n
            .checked_mul(d)
            .ok_or_else(|| JetComputeError::InvalidShape("Tensor numel overflow".to_string()))?;
    }
    Ok(n)
}

fn jet_compute_storage_len(shape: &[i64]) -> Result<usize, JetComputeError> {
    // Validate strides even when an earlier zero axis makes the element count
    // zero. Otherwise a shape such as `[0, i64::MAX]` could look empty while
    // carrying an unrepresentable axis into later indexing code.
    jet_compute_row_major_strides(shape)?;
    let n = jet_compute_numel(shape)?;
    let len = usize::try_from(n).map_err(|_| {
        JetComputeError::InvalidShape("Tensor storage length is too large".to_string())
    })?;
    if len > MAX_TENSOR_ELEMENTS {
        return Err(JetComputeError::InvalidShape(format!(
            "Tensor storage exceeds the {}-element Core limit",
            MAX_TENSOR_ELEMENTS
        )));
    }
    Ok(len)
}

fn jet_compute_view_metadata(
    tensor: &JetTensor,
) -> Result<(&[i64], usize), JetComputeError> {
    let rank = tensor.shape.len();
    if tensor.strides.len() == rank {
        return Ok((&tensor.strides, 0));
    }
    if rank.checked_add(1) != Some(tensor.strides.len()) {
        return Err(JetComputeError::InvalidShape(
            "Tensor stride and view metadata disagree".to_string(),
        ));
    }
    let offset = usize::try_from(tensor.strides[rank]).map_err(|_| {
        JetComputeError::InvalidShape("Tensor view offset must be non-negative".to_string())
    })?;
    Ok((&tensor.strides[..rank], offset))
}

fn jet_compute_view_strides(
    shape: &[i64],
    offset: usize,
) -> Result<Vec<i64>, JetComputeError> {
    let mut strides = jet_compute_row_major_strides(shape)?;
    if offset != 0 {
        strides.push(i64::try_from(offset).map_err(|_| {
            JetComputeError::InvalidShape("Tensor view offset is too large".to_string())
        })?);
    }
    Ok(strides)
}

fn jet_compute_tensor_view_bounds(
    tensor: &JetTensor,
    offset: usize,
    expected_len: usize,
) -> Result<std::ops::Range<usize>, JetComputeError> {
    let expected_strides = jet_compute_row_major_strides(&tensor.shape)?;
    let (strides, metadata_offset) = jet_compute_view_metadata(tensor)?;
    if strides != expected_strides || metadata_offset != offset {
        return Err(JetComputeError::Unsupported(
            "this operation requires a contiguous Tensor view".to_string(),
        ));
    }
    let end = offset.checked_add(expected_len).ok_or_else(|| {
        JetComputeError::InvalidShape("Tensor view end overflows backing storage".to_string())
    })?;
    if end > tensor.data.len() {
        return Err(JetComputeError::InvalidShape(
            "Tensor view exceeds backing storage".to_string(),
        ));
    }
    Ok(offset..end)
}

fn jet_compute_view_storage_end(
    tensor: &JetTensor,
    strides: &[i64],
    offset: usize,
) -> Result<usize, JetComputeError> {
    let mut relative_end = 0usize;
    for (&dim, &stride) in tensor.shape.iter().zip(strides.iter()) {
        if dim == 0 {
            continue;
        }
        let dim = usize::try_from(dim).map_err(|_| {
            JetComputeError::InvalidShape("Tensor shape axis is too large".to_string())
        })?;
        let stride = usize::try_from(stride).map_err(|_| {
            JetComputeError::InvalidShape(
                "Tensor view strides must be non-negative and representable".to_string(),
            )
        })?;
        let extent = dim.checked_sub(1).and_then(|last| last.checked_mul(stride)).ok_or_else(|| {
            JetComputeError::InvalidShape("Tensor view extent overflows backing storage".to_string())
        })?;
        relative_end = relative_end.checked_add(extent).ok_or_else(|| {
            JetComputeError::InvalidShape("Tensor view extent overflows backing storage".to_string())
        })?;
    }
    offset.checked_add(relative_end).and_then(|end| end.checked_add(1)).ok_or_else(|| {
        JetComputeError::InvalidShape("Tensor view end overflows backing storage".to_string())
    })
}

fn jet_compute_tensor_values(tensor: &JetTensor) -> Vec<f64> {
    let Ok(expected_len) = jet_compute_storage_len(&tensor.shape) else {
        return Vec::new();
    };
    let Ok((strides, offset)) = jet_compute_view_metadata(tensor) else {
        return Vec::new();
    };
    let data = tensor.data.as_ref();
    if expected_len == 0 {
        return Vec::new();
    }
    let mut values = Vec::with_capacity(expected_len);
    for flat in 0..expected_len {
        let mut remainder = flat;
        let mut relative_offset = 0usize;
        for axis in (0..tensor.shape.len()).rev() {
            let dim = match usize::try_from(tensor.shape[axis]) {
                Ok(dim) if dim != 0 => dim,
                _ => return Vec::new(),
            };
            let index = remainder % dim;
            remainder /= dim;
            let stride = match usize::try_from(strides[axis]) {
                Ok(stride) => stride,
                Err(_) => return Vec::new(),
            };
            let term = match index.checked_mul(stride) {
                Some(term) => term,
                None => return Vec::new(),
            };
            relative_offset = match relative_offset.checked_add(term) {
                Some(offset) => offset,
                None => return Vec::new(),
            };
        }
        let physical_offset = match offset.checked_add(relative_offset) {
            Some(offset) => offset,
            None => return Vec::new(),
        };
        let Some(value) = data.get(physical_offset).copied() else {
            return Vec::new();
        };
        values.push(value);
    }
    values
}

fn jet_compute_validate_placement(
    device: JetComputeDevice,
    receipt: &JetComputePlacementReceipt,
) -> Result<(), JetComputeError> {
    if device == JetComputeDevice::Auto || receipt.selected == JetComputeDevice::Auto {
        return Err(JetComputeError::Unsupported(
            "a Tensor must record the concrete backend selected by Auto placement".to_string(),
        ));
    }
    let Some(expected_abilities) =
        jet_compute_registered_backend_abilities(&receipt.backend, &receipt.profile)
    else {
        return Err(JetComputeError::Unsupported(format!(
            "compute backend `{}` does not register profile `{}`",
            receipt.backend, receipt.profile
        )));
    };
    let registered = match receipt.selected {
        JetComputeDevice::Cpu => {
            receipt.backend == CPU_ORACLE_BACKEND
                && receipt.version == CPU_ORACLE_VERSION
                && receipt.cache == CPU_ORACLE_CACHE
                && matches!(
                    (receipt.requested, receipt.selected),
                    (JetComputeDevice::Cpu, JetComputeDevice::Cpu)
                        | (JetComputeDevice::Auto, JetComputeDevice::Cpu)
                )
        }
        JetComputeDevice::Metal => {
            receipt.backend == METAL_BACKEND
                && receipt.version == METAL_VERSION
                && receipt.cache == METAL_CACHE
                && receipt.requested == JetComputeDevice::Metal
        }
        JetComputeDevice::Auto => false,
    };
    if device != receipt.selected
        || !registered
        || receipt.reason.is_empty()
        || receipt.reason.chars().any(char::is_control)
        || !jet_compute_abilities_match(&receipt.abilities, expected_abilities)
    {
        return Err(JetComputeError::Device(
            "Tensor placement receipt does not match a registered backend ability".to_string(),
        ));
    }
    Ok(())
}

fn jet_compute_validate_tensor(tensor: &JetTensor) -> Result<(), JetComputeError> {
    jet_compute_validate_placement(tensor.device, &tensor.last_placement)?;
    let expected_len = jet_compute_storage_len(&tensor.shape)?;
    let (strides, offset) = jet_compute_view_metadata(tensor)?;
    if strides.iter().any(|stride| *stride < 0) {
        return Err(JetComputeError::InvalidShape(
            "Tensor view strides must be non-negative".to_string(),
        ));
    }
    if strides
        .iter()
        .zip(tensor.shape.iter())
        .any(|(stride, dim)| *dim > 1 && *stride == 0)
    {
        return Err(JetComputeError::Unsupported(
            "zero-stride Tensor views are not writable aliases".to_string(),
        ));
    }
    if expected_len == 0 {
        if offset > tensor.data.len() {
            return Err(JetComputeError::InvalidShape(
                "empty Tensor view starts outside backing storage".to_string(),
            ));
        }
        return Ok(());
    }
    let storage_end = jet_compute_view_storage_end(tensor, strides, offset)?;
    if storage_end > tensor.data.len() {
        return Err(JetComputeError::InvalidShape(
            "Tensor view exceeds backing storage".to_string(),
        ));
    }
    let values = jet_compute_tensor_values(tensor);
    if values.len() != expected_len {
        return Err(JetComputeError::InvalidShape(
            "Tensor view metadata does not address its logical storage".to_string(),
        ));
    }
    if values.iter().any(|value| !value.is_finite()) {
        return Err(JetComputeError::Arithmetic(
            "Tensor values must be finite".to_string(),
        ));
    }
    for value in &values {
        jet_compute_validate_profile_value(
            &tensor.last_placement.profile,
            *value,
            "Tensor value",
        )?;
    }
    Ok(())
}

fn jet_compute_place_with_profile(
    requested: JetComputeDevice,
    profile: &str,
) -> Result<JetComputePlacementReceipt, JetComputeError> {
    let selected = match requested {
        JetComputeDevice::Auto | JetComputeDevice::Cpu => JetComputeDevice::Cpu,
        JetComputeDevice::Metal => {
            if profile != CPU_ORACLE_F32_PROFILE {
                return Err(JetComputeError::Unsupported(
                    "Metal backend supports only F32Strict+Reproducible; create an F32 Tensor first"
                        .to_string(),
                ));
            }
            if !jet_compute_metal::available() {
                return Err(JetComputeError::Device(
                    "Metal device is unavailable; no CPU fallback was selected".to_string(),
                ));
            }
            JetComputeDevice::Metal
        }
    };
    let backend = if selected == JetComputeDevice::Metal {
        METAL_BACKEND
    } else {
        CPU_ORACLE_BACKEND
    };
    let version = if selected == JetComputeDevice::Metal {
        METAL_VERSION
    } else {
        CPU_ORACLE_VERSION
    };
    let cache = if selected == JetComputeDevice::Metal {
        METAL_CACHE
    } else {
        CPU_ORACLE_CACHE
    };
    let profile = if selected == JetComputeDevice::Metal {
        CPU_ORACLE_F32_PROFILE
    } else {
        profile
    };
    let abilities = jet_compute_registered_backend_abilities(backend, profile)
        .ok_or_else(|| JetComputeError::Unsupported("compute profile is not registered".to_string()))?
        .iter()
        .map(|ability| (*ability).to_string())
        .collect();
    let ability = if selected == JetComputeDevice::Metal {
        "metal.f32"
    } else if profile == CPU_ORACLE_F32_PROFILE {
        "cpu-oracle.f32"
    } else {
        "cpu-oracle.f64"
    };
    Ok(JetComputePlacementReceipt {
        requested,
        selected,
        backend: backend.to_string(),
        version: version.to_string(),
        profile: profile.to_string(),
        cache: cache.to_string(),
        abilities,
        reason: if requested == JetComputeDevice::Auto {
            format!("policy=auto; selected=cpu; ability={ability}")
        } else if selected == JetComputeDevice::Metal {
            "policy=explicit; selected=metal; ability=metal.f32".to_string()
        } else {
            format!("policy=explicit; selected=cpu; ability={ability}")
        },
    })
}

fn jet_compute_place(
    requested: JetComputeDevice,
) -> Result<JetComputePlacementReceipt, JetComputeError> {
    jet_compute_place_with_profile(requested, CPU_ORACLE_F64_PROFILE)
}

fn jet_compute_inherit_placement(mut tensor: JetTensor, source: &JetTensor) -> JetTensor {
    tensor.device = source.device;
    tensor.last_placement = source.last_placement.clone();
    tensor.last_transfer = None;
    tensor
}

fn jet_compute_tensor_from_shape_like(
    source: &JetTensor,
    shape: Vec<i64>,
    fill: f64,
) -> Result<JetTensor, JetComputeError> {
    Ok(jet_compute_inherit_placement(
        jet_compute_tensor_from_shape(shape, fill, JetComputeDevice::Cpu)?,
        source,
    ))
}

fn jet_compute_metal_values(
    tensor: &JetTensor,
    context: &str,
) -> Result<Vec<f32>, JetComputeError> {
    if tensor.device != JetComputeDevice::Metal {
        return Err(JetComputeError::Device(format!(
            "{context} requires a Metal Tensor"
        )));
    }
    if tensor.last_placement.profile != CPU_ORACLE_F32_PROFILE {
        return Err(JetComputeError::Unsupported(format!(
            "Metal {context} supports only F32Strict+Reproducible"
        )));
    }
    if !jet_compute_metal::available() {
        return Err(JetComputeError::Device(
            "Metal device was lost before launch".to_string(),
        ));
    }
    jet_compute_tensor_values(tensor)
        .into_iter()
        .map(|value| jet_compute_f32_value(value, context))
        .collect()
}

fn jet_compute_metal_result_like(
    source: &JetTensor,
    shape: Vec<i64>,
    values: Vec<f32>,
) -> Result<JetTensor, JetComputeError> {
    let expected = jet_compute_storage_len(&shape)?;
    if values.len() != expected {
        return Err(JetComputeError::InvalidShape(
            "Metal kernel returned the wrong storage length".to_string(),
        ));
    }
    let mut output = jet_compute_tensor_from_shape_like(source, shape.clone(), 0.0)?;
    output.strides = jet_compute_row_major_strides(&shape)?;
    output.data = std::sync::Arc::new(values.into_iter().map(f64::from).collect());
    jet_compute_validate_tensor(&output)?;
    Ok(output)
}

fn jet_compute_metal_binary_values(
    op: &str,
    left: &[f32],
    right: &[f32],
) -> Result<Vec<f32>, JetComputeError> {
    let op = match op {
        "add" => 0,
        "mul" => 1,
        "sub" => 2,
        "div" => 3,
        "maximum" => 4,
        "minimum" => 5,
        _ => {
            return Err(JetComputeError::Unsupported(format!(
                "unsupported Metal binary operation `{op}`"
            )))
        }
    };
    jet_compute_metal::binary(op, left, right)
}

fn jet_compute_metal_unary_values(
    op: &str,
    values: &[f32],
) -> Result<Vec<f32>, JetComputeError> {
    let op = match op {
        "negate" => 0,
        "abs" => 1,
        "exp" => 2,
        "log" => 3,
        "sqrt" => 4,
        _ => {
            return Err(JetComputeError::Unsupported(format!(
                "unsupported Metal unary operation `{op}`"
            )))
        }
    };
    jet_compute_metal::unary(op, values)
}

fn jet_compute_require_same_contract(
    left: &JetTensor,
    right: &JetTensor,
    operation: &str,
) -> Result<(), JetComputeError> {
    if left.device != right.device {
        return Err(JetComputeError::Device(format!(
            "{operation} tensors must use the same device"
        )));
    }
    if left.last_placement.profile != right.last_placement.profile {
        return Err(JetComputeError::Device(format!(
            "{operation} tensors must use the same precision profile"
        )));
    }
    Ok(())
}

fn jet_compute_tensor_from_shape(
    shape: Vec<i64>,
    fill: f64,
    requested: JetComputeDevice,
) -> Result<JetTensor, JetComputeError> {
    if !fill.is_finite() {
        return Err(JetComputeError::Arithmetic(
            "Tensor values must be finite".to_string(),
        ));
    }
    let strides = jet_compute_row_major_strides(&shape)?;
    let n = jet_compute_storage_len(&shape)?;
    let receipt = jet_compute_place(requested)?;
    Ok(JetTensor {
        shape,
        strides,
        data: std::sync::Arc::new(vec![fill; n]),
        device: receipt.selected,
        last_placement: receipt,
        last_transfer: None,
        trace: None,
    })
}

fn jet_compute_zeros(shape: &Vec<i64>) -> Result<JetTensor, JetComputeError> {
    jet_compute_tensor_from_shape(shape.clone(), 0.0, JetComputeDevice::Auto)
}

fn jet_compute_ones(shape: &Vec<i64>) -> Result<JetTensor, JetComputeError> {
    jet_compute_tensor_from_shape(shape.clone(), 1.0, JetComputeDevice::Auto)
}

fn jet_compute_full(shape: &Vec<i64>, value: f64) -> Result<JetTensor, JetComputeError> {
    jet_compute_tensor_from_shape(shape.clone(), value, JetComputeDevice::Auto)
}

fn jet_compute_from_list(values: &Vec<f64>) -> Result<JetTensor, JetComputeError> {
    if values.iter().any(|value| !value.is_finite()) {
        return Err(JetComputeError::Arithmetic(
            "Tensor values must be finite".to_string(),
        ));
    }
    let len = i64::try_from(values.len()).map_err(|_| {
        JetComputeError::InvalidShape("Tensor list is too long".to_string())
    })?;
    let shape = vec![len];
    let strides = jet_compute_row_major_strides(&shape)?;
    let storage_len = jet_compute_storage_len(&shape)?;
    let receipt = jet_compute_place(JetComputeDevice::Auto)?;
    Ok(JetTensor {
        shape,
        strides,
        data: std::sync::Arc::new(values[..storage_len].to_vec()),
        device: receipt.selected,
        last_placement: receipt,
        last_transfer: None,
        trace: None,
    })
}

fn jet_compute_copy_checked(tensor: &JetTensor) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    let shape = tensor.shape.clone();
    let copy = JetTensor {
        strides: jet_compute_row_major_strides(&shape)?,
        data: std::sync::Arc::new(jet_compute_tensor_values(tensor)),
        shape,
        device: tensor.device,
        last_placement: tensor.last_placement.clone(),
        last_transfer: tensor.last_transfer.clone(),
        trace: tensor.trace.clone(),
    };
    jet_compute_validate_tensor(&copy)?;
    Ok(copy)
}

/// Replace the ambient list projection after a mutable view write-back. The
/// adapter supplies only the already-marshalled values; storage exclusivity,
/// mutation policy, and the resulting canonical row-major metadata remain in
/// the shared Prelude.
fn jet_compute_replace_data_checked(
    tensor: &mut JetTensor,
    values: Vec<f64>,
) -> Result<(), JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    if tensor.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "Tensor mutation is not differentiable; use a pure Tensor function".to_string(),
        ));
    }
    let expected = jet_compute_storage_len(&tensor.shape)?;
    if values.len() != expected {
        return Err(JetComputeError::InvalidShape(
            "Tensor write-back length does not match its shape".to_string(),
        ));
    }
    let Some(data) = std::sync::Arc::get_mut(&mut tensor.data) else {
        return Err(JetComputeError::Unsupported(
            "Tensor mutable view requires exclusive backing storage".to_string(),
        ));
    };
    *data = values;
    tensor.strides = jet_compute_row_major_strides(&tensor.shape)?;
    jet_compute_validate_tensor(tensor)
}

fn jet_compute_copy(tensor: &JetTensor) -> JetTensor {
    match jet_compute_copy_checked(tensor) {
        Ok(copy) => copy,
        Err(error) => jet_panic("Compute.rs", line!(), &error.jet_show()),
    }
}

/// Return the flat storage range selected by a bracket range.  A Tensor range
/// selects rows on its first axis, so a rank-2 matrix window keeps complete
/// rows and a higher-rank window keeps complete first-axis slabs.  This is the
/// one-dimensional bracket surface ratified by D-SHAPE-PLACE1; the inner axes
/// remain part of the same contiguous storage projection.
fn jet_compute_window_bounds(
    tensor: &JetTensor,
    start: i64,
    end: i64,
    exclusive: bool,
) -> Result<std::ops::Range<usize>, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    let (strides, offset) = jet_compute_view_metadata(tensor)?;
    let expected_strides = jet_compute_row_major_strides(&tensor.shape)?;
    if strides != expected_strides {
        return Err(JetComputeError::Unsupported(
            "Tensor window is non-contiguous; use the strided View projection".to_string(),
        ));
    }
    let axis_len = tensor.shape.first().copied().ok_or_else(|| {
        JetComputeError::InvalidShape("Tensor shape must have at least one axis".to_string())
    })?;
    let Some((axis_start, axis_end)) = jet_range_bounds(start, end, exclusive, axis_len) else {
        return Err(JetComputeError::OutOfBounds(format!(
            "Tensor range {}{}{} is outside first axis of extent {}",
            start,
            if exclusive { "..<" } else { ".." },
            end,
            axis_len
        )));
    };
    // In row-major storage the first stride is the number of scalar values in
    // one first-axis slab.  For an empty first axis the selected range is also
    // empty, so the stride is still safe to use.
    let slab = strides.first().copied().ok_or_else(|| {
        JetComputeError::InvalidShape("Tensor is missing its first-axis stride".to_string())
    })?;
    let flat_start = axis_start.checked_mul(slab).ok_or_else(|| {
        JetComputeError::OutOfBounds("Tensor view start overflows storage".to_string())
    })?;
    let flat_end = axis_end.checked_mul(slab).ok_or_else(|| {
        JetComputeError::OutOfBounds("Tensor view end overflows storage".to_string())
    })?;
    let start = usize::try_from(flat_start).map_err(|_| {
        JetComputeError::OutOfBounds("Tensor view start is outside storage".to_string())
    })?;
    let end = usize::try_from(flat_end).map_err(|_| {
        JetComputeError::OutOfBounds("Tensor view end is outside storage".to_string())
    })?;
    let start = offset.checked_add(start).ok_or_else(|| {
        JetComputeError::OutOfBounds("Tensor view start overflows storage".to_string())
    })?;
    let end = offset.checked_add(end).ok_or_else(|| {
        JetComputeError::OutOfBounds("Tensor view end overflows storage".to_string())
    })?;
    if end > tensor.data.len() || start > end {
        return Err(JetComputeError::OutOfBounds(
            "Tensor view is outside storage".to_string(),
        ));
    }
    Ok(start..end)
}

fn jet_compute_slice_checked(
    tensor: &JetTensor,
    start: i64,
    end: i64,
    exclusive: bool,
) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    if tensor.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "Tensor views are not differentiable; reshape or copy before transforming".to_string(),
        ));
    }
    let axis_len = tensor.shape.first().copied().ok_or_else(|| {
        JetComputeError::InvalidShape("Tensor shape must have at least one axis".to_string())
    })?;
    let Some((axis_start, axis_end)) = jet_range_bounds(start, end, exclusive, axis_len) else {
        return Err(JetComputeError::OutOfBounds(format!(
            "Tensor range {}{}{} is outside first axis of extent {}",
            start,
            if exclusive { "..<" } else { ".." },
            end,
            axis_len
        )));
    };
    let mut shape = tensor.shape.clone();
    shape[0] = axis_end.checked_sub(axis_start).ok_or_else(|| {
        JetComputeError::OutOfBounds("Tensor slice has a negative extent".to_string())
    })?;
    let (source_strides, base_offset) = jet_compute_view_metadata(tensor)?;
    let first_stride = source_strides.first().copied().ok_or_else(|| {
        JetComputeError::InvalidShape("Tensor is missing its first-axis stride".to_string())
    })?;
    let first_stride = usize::try_from(first_stride).map_err(|_| {
        JetComputeError::InvalidShape("Tensor view stride is not representable".to_string())
    })?;
    let axis_start = usize::try_from(axis_start).map_err(|_| {
        JetComputeError::OutOfBounds("Tensor slice start is not representable".to_string())
    })?;
    let start_offset = base_offset
        .checked_add(axis_start.checked_mul(first_stride).ok_or_else(|| {
            JetComputeError::OutOfBounds("Tensor slice start overflows storage".to_string())
        })?)
        .ok_or_else(|| JetComputeError::OutOfBounds("Tensor slice start overflows storage".to_string()))?;
    let mut view_strides = source_strides.to_vec();
    if start_offset != 0 {
        view_strides.push(i64::try_from(start_offset).map_err(|_| {
            JetComputeError::InvalidShape("Tensor view offset is too large".to_string())
        })?);
    }
    // An owned bracket slice is an ownership conversion, not another view.
    // Read the selected logical values once, then give the result independent
    // row-major storage. Read-only and mutable view helpers above retain their
    // zero-copy backing; only this owned path detaches it.
    let view = JetTensor {
        shape,
        strides: view_strides,
        data: tensor.data.clone(),
        device: tensor.device,
        last_placement: tensor.last_placement.clone(),
        last_transfer: tensor.last_transfer.clone(),
        trace: tensor.trace.clone(),
    };
    let slice = JetTensor {
        shape: view.shape.clone(),
        strides: jet_compute_row_major_strides(&view.shape)?,
        data: std::sync::Arc::new(jet_compute_tensor_values(&view)),
        device: view.device,
        last_placement: view.last_placement.clone(),
        last_transfer: view.last_transfer.clone(),
        trace: view.trace.clone(),
    };
    jet_compute_validate_tensor(&slice)?;
    Ok(slice)
}

fn jet_compute_slice(
    tensor: &JetTensor,
    start: i64,
    end: i64,
    exclusive: bool,
    file: &str,
    line: u32,
) -> JetTensor {
    match jet_compute_slice_checked(tensor, start, end, exclusive) {
        Ok(slice) => slice,
        Err(error) => jet_panic(file, line, &error.jet_show()),
    }
}

fn jet_compute_slice_range(
    tensor: &JetTensor,
    range: &JetRange,
    file: &str,
    line: u32,
) -> JetTensor {
    jet_compute_slice(tensor, range.start, range.end, range.exclusive, file, line)
}

fn jet_compute_view_checked<'a>(
    tensor: &'a JetTensor,
    start: i64,
    end: i64,
    exclusive: bool,
) -> Result<&'a [f64], JetComputeError> {
    if tensor.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "Tensor views are not differentiable; reshape or copy before transforming".to_string(),
        ));
    }
    let bounds = jet_compute_window_bounds(tensor, start, end, exclusive)?;
    Ok(&tensor.data[bounds])
}

fn jet_compute_view<'a>(
    tensor: &'a JetTensor,
    start: i64,
    end: i64,
    exclusive: bool,
    file: &str,
    line: u32,
) -> &'a [f64] {
    match jet_compute_view_checked(tensor, start, end, exclusive) {
        Ok(view) => view,
        Err(error) => jet_panic(file, line, &error.jet_show()),
    }
}

fn jet_compute_view_range<'a>(
    tensor: &'a JetTensor,
    range: &JetRange,
    file: &str,
    line: u32,
) -> &'a [f64] {
    jet_compute_view(tensor, range.start, range.end, range.exclusive, file, line)
}

fn jet_compute_view_mut_checked<'a>(
    tensor: &'a mut JetTensor,
    start: i64,
    end: i64,
    exclusive: bool,
) -> Result<JetComputeViewMut<'a>, JetComputeError> {
    if tensor.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "Tensor mutation is not differentiable; use a pure Tensor function".to_string(),
        ));
    }
    jet_compute_window_bounds(tensor, start, end, exclusive)?;
    if std::sync::Arc::get_mut(&mut tensor.data).is_none() {
        return Err(JetComputeError::Unsupported(
            "Tensor mutable view requires exclusive backing storage".to_string(),
        ));
    }
    Ok(JetComputeViewMut {
        tensor,
        start,
        end,
        exclusive,
    })
}

fn jet_compute_view_mut<'a>(
    tensor: &'a mut JetTensor,
    start: i64,
    end: i64,
    exclusive: bool,
    file: &str,
    line: u32,
) -> JetComputeViewMut<'a> {
    match jet_compute_view_mut_checked(tensor, start, end, exclusive) {
        Ok(view) => view,
        Err(error) => jet_panic(file, line, &error.jet_show()),
    }
}

fn jet_compute_view_mut_range<'a>(
    tensor: &'a mut JetTensor,
    range: &JetRange,
    file: &str,
    line: u32,
) -> JetComputeViewMut<'a> {
    jet_compute_view_mut(tensor, range.start, range.end, range.exclusive, file, line)
}

fn jet_compute_window_set_view(
    view: &mut JetComputeViewMut<'_>,
    index: i64,
    value: f64,
) -> Result<(), String> {
    jet_compute_window_set(
        view.tensor,
        view.start,
        view.end,
        view.exclusive,
        index,
        value,
    )
}

fn jet_compute_window_get_view(
    view: &JetComputeViewMut<'_>,
    index: i64,
    file: &str,
    line: u32,
) -> f64 {
    match jet_compute_window_get(
        view.tensor,
        view.start,
        view.end,
        view.exclusive,
        index,
    ) {
        Ok(value) => value,
        Err(error) => jet_panic(file, line, &error),
    }
}

impl<'a> JetComputeViewMut<'a> {
    fn len(&self) -> i64 {
        match jet_compute_window_bounds(self.tensor, self.start, self.end, self.exclusive) {
            Ok(bounds) => i64::try_from(bounds.len()).unwrap_or(i64::MAX),
            Err(error) => jet_panic("Compute.rs", line!(), &error.jet_show()),
        }
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn to_vec(&self) -> Vec<f64> {
        match jet_compute_view_checked(self.tensor, self.start, self.end, self.exclusive) {
            Ok(view) => view.to_vec(),
            Err(error) => jet_panic("Compute.rs", line!(), &error.jet_show()),
        }
    }
}

/// The one mutable Tensor-window write seam. Every engine supplies the Tensor
/// handle, the original window bounds, and the logical element coordinate;
/// this Prelude operation owns trace policy, exclusive backing storage,
/// window bounds, element addressing, finite-value validation, mutation, and
/// their canonical errors.
fn jet_compute_window_set(
    tensor: &mut JetTensor,
    start: i64,
    end: i64,
    exclusive: bool,
    index: i64,
    value: f64,
) -> Result<(), String> {
    if tensor.trace.is_some() {
        return Err("Tensor mutation is not differentiable; use a pure Tensor function".to_string());
    }
    if !value.is_finite() {
        return Err("Tensor values must be finite".to_string());
    }
    jet_compute_validate_profile_value(
        &tensor.last_placement.profile,
        value,
        "Tensor write value",
    )
    .map_err(|error| error.jet_show())?;
    let bounds = jet_compute_window_bounds(tensor, start, end, exclusive)
        .map_err(|error| error.jet_show())?;
    // Validate the logical element before exclusivity. A valid empty window is
    // still a view; an attempted element write must therefore report the
    // canonical element-bounds error even when the owner has another handle.
    let relative = jet_view_address(bounds.len(), index)?;
    let Some(data) = std::sync::Arc::get_mut(&mut tensor.data) else {
        return Err("Tensor mutable view requires exclusive backing storage".to_string());
    };
    let offset = bounds
        .start
        .checked_add(relative)
        .ok_or_else(|| "Tensor view index is outside storage".to_string())?;
    let Some(slot) = data.get_mut(offset) else {
        return Err("Tensor view index is outside storage".to_string());
    };
    *slot = value;
    Ok(())
}

/// The matching checked read for a Tensor window. It keeps element addressing
/// (including empty-window rejection) beside the mutable write seam.
fn jet_compute_window_get(
    tensor: &JetTensor,
    start: i64,
    end: i64,
    exclusive: bool,
    index: i64,
) -> Result<f64, String> {
    let view = jet_compute_view_checked(tensor, start, end, exclusive)
        .map_err(|error| error.jet_show())?;
    jet_view_get_checked(view, index)
}

fn jet_compute_tensor_shape(tensor: &JetTensor) -> Vec<i64> {
    tensor.shape.clone()
}

fn jet_compute_tensor_rank(tensor: &JetTensor) -> i64 {
    i64::try_from(tensor.shape.len()).unwrap_or(i64::MAX)
}

fn jet_compute_tensor_numel(tensor: &JetTensor) -> i64 {
    i64::try_from(jet_compute_tensor_values(tensor).len()).unwrap_or(i64::MAX)
}

fn jet_compute_tensor_device(tensor: &JetTensor) -> String {
    tensor.device.jet_show()
}

fn jet_compute_tensor_placement(tensor: &JetTensor) -> String {
    tensor.last_placement.jet_show()
}

fn jet_compute_tensor_to_list(tensor: &JetTensor) -> Vec<f64> {
    if tensor.trace.is_some() {
        jet_panic(
            "Compute.rs",
            line!(),
            "Tensor value reads have no registered autodiff rule",
        );
    }
    jet_compute_tensor_values(tensor)
}

fn jet_compute_offset(tensor: &JetTensor, indices: &[i64]) -> Result<usize, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    if indices.len() != tensor.shape.len() {
        return Err(JetComputeError::RankMismatch(format!(
            "expected {} indices, got {}",
            tensor.shape.len(),
            indices.len()
        )));
    }
    let (strides, base_offset) = jet_compute_view_metadata(tensor)?;
    let mut relative_offset = 0i64;
    for (i, (&idx, (&dim, &stride))) in indices
        .iter()
        .zip(tensor.shape.iter().zip(strides.iter()))
        .enumerate()
    {
        if idx < 0 || idx >= dim {
            return Err(JetComputeError::OutOfBounds(format!(
                "index {} out of range for axis {} of extent {}",
                idx, i, dim
            )));
        }
        let term = idx.checked_mul(stride).ok_or_else(|| {
            JetComputeError::OutOfBounds("tensor index offset overflow".to_string())
        })?;
        relative_offset = relative_offset.checked_add(term).ok_or_else(|| {
            JetComputeError::OutOfBounds("tensor index offset overflow".to_string())
        })?;
    }
    usize::try_from(relative_offset)
        .ok()
        .and_then(|relative| base_offset.checked_add(relative))
        .filter(|index| *index < tensor.data.len())
        .ok_or_else(|| JetComputeError::OutOfBounds("tensor index is outside storage".to_string()))
}

fn jet_compute_get_raw(tensor: &JetTensor, indices: &[i64]) -> Result<f64, JetComputeError> {
    let offset = jet_compute_offset(tensor, indices)?;
    tensor.data.get(offset).ok_or_else(|| {
        JetComputeError::OutOfBounds("tensor index is outside storage".to_string())
    }).copied()
}

fn jet_compute_get(tensor: &JetTensor, indices: &[i64]) -> Result<f64, JetComputeError> {
    if tensor.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "Tensor element reads have no registered autodiff rule".to_string(),
        ));
    }
    jet_compute_get_raw(tensor, indices)
}

impl JetComputeSetTarget for JetTensor {
    type Error = JetComputeError;

    fn jet_compute_set_target(
        &mut self,
        indices: &[i64],
        value: f64,
    ) -> Result<(), JetComputeError> {
        if self.trace.is_some() {
            return Err(JetComputeError::Unsupported(
                "Tensor mutation is not differentiable; use a pure Tensor function".to_string(),
            ));
        }
        if !value.is_finite() {
            return Err(JetComputeError::Arithmetic(
                "Tensor values must be finite".to_string(),
            ));
        }
        jet_compute_validate_profile_value(
            &self.last_placement.profile,
            value,
            "Tensor write value",
        )?;
        let offset = jet_compute_offset(self, indices)?;
        let Some(data) = std::sync::Arc::get_mut(&mut self.data) else {
            return Err(JetComputeError::Unsupported(
                "Tensor write requires an exclusive ViewMut borrow".to_string(),
            ));
        };
        let Some(slot) = data.get_mut(offset) else {
            return Err(JetComputeError::OutOfBounds(
                "tensor index is outside storage".to_string(),
            ));
        };
        *slot = value;
        Ok(())
    }
}

fn jet_compute_add(a: &JetTensor, b: &JetTensor) -> Result<JetTensor, JetComputeError> {
    jet_compute_binary("add", a, b)
}

fn jet_compute_mul(a: &JetTensor, b: &JetTensor) -> Result<JetTensor, JetComputeError> {
    jet_compute_binary("mul", a, b)
}

fn jet_compute_reshape(
    tensor: &JetTensor,
    shape: &Vec<i64>,
) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    let n = jet_compute_storage_len(shape)?;
    let tensor_n = jet_compute_storage_len(&tensor.shape)?;
    if n != tensor_n {
        return Err(JetComputeError::InvalidShape(format!(
            "reshape numel {} does not match tensor numel {}",
            n,
            tensor_n
        )));
    }
    let (source_strides, offset) = jet_compute_view_metadata(tensor)?;
    let source_row_major = jet_compute_row_major_strides(&tensor.shape)?;
    let (strides, data) = if source_strides == source_row_major {
        (jet_compute_view_strides(shape, offset)?, tensor.data.clone())
    } else {
        // Reshape is an explicit rank/layout conversion. A non-contiguous view
        // cannot be relabeled as contiguous storage, so materialize its logical
        // order into a new owner.
        (
            jet_compute_row_major_strides(shape)?,
            std::sync::Arc::new(jet_compute_tensor_values(tensor)),
        )
    };
    let output = JetTensor {
        shape: shape.clone(),
        strides,
        data,
        device: tensor.device,
        last_placement: tensor.last_placement.clone(),
        last_transfer: None,
        trace: None,
    };
    jet_compute_record(
        output,
        &[tensor],
        vec![tensor.clone()],
        JetComputeTapeRule::Reshape {
            source_shape: tensor.shape.clone(),
        },
    )
}

/// Matrix alias: rank-2 Tensor sharing the same storage law (D-COMPUTE-TYPE1).
fn jet_compute_matrix(rows: i64, cols: i64, fill: f64) -> Result<JetTensor, JetComputeError> {
    if rows < 0 || cols < 0 {
        return Err(JetComputeError::InvalidShape(
            "Matrix rows and cols must be non-negative".to_string(),
        ));
    }
    jet_compute_tensor_from_shape(vec![rows, cols], fill, JetComputeDevice::Cpu)
}

/// Vec alias: rank-1 Tensor sharing the same storage law (D-COMPUTE-TYPE1).
fn jet_compute_vec(len: i64, fill: f64) -> Result<JetTensor, JetComputeError> {
    if len < 0 {
        return Err(JetComputeError::InvalidShape(
            "Vec length must be non-negative".to_string(),
        ));
    }
    jet_compute_tensor_from_shape(vec![len], fill, JetComputeDevice::Cpu)
}

fn jet_compute_matmul(a: &JetTensor, b: &JetTensor) -> Result<JetTensor, JetComputeError> {
    if a.shape.len() != 2 || b.shape.len() != 2 {
        return Err(JetComputeError::RankMismatch(
            "matmul requires rank-2 tensors".to_string(),
        ));
    }
    jet_compute_validate_tensor(a)?;
    jet_compute_validate_tensor(b)?;
    jet_compute_require_same_contract(a, b, "matmul")?;
    let (m, k) = (a.shape[0], a.shape[1]);
    let (k2, n) = (b.shape[0], b.shape[1]);
    if m < 0 || k < 0 || k2 < 0 || n < 0 {
        return Err(JetComputeError::InvalidShape(
            "matmul dimensions must be non-negative".to_string(),
        ));
    }
    if k != k2 {
        return Err(JetComputeError::RankMismatch(format!(
            "matmul inner dims {} and {} disagree",
            k, k2
        )));
    }
    if a.device == JetComputeDevice::Metal {
        let rows = usize::try_from(m)
            .map_err(|_| JetComputeError::InvalidShape("Metal matmul rows are too large".to_string()))?;
        let inner = usize::try_from(k)
            .map_err(|_| JetComputeError::InvalidShape("Metal matmul inner dimension is too large".to_string()))?;
        let cols = usize::try_from(n)
            .map_err(|_| JetComputeError::InvalidShape("Metal matmul columns are too large".to_string()))?;
        let left = jet_compute_metal_values(a, "matmul input")?;
        let right = jet_compute_metal_values(b, "matmul input")?;
        let data = jet_compute_metal::matmul(&left, &right, rows, inner, cols)?;
        return jet_compute_record(
            jet_compute_metal_result_like(a, vec![m, n], data)?,
            &[a, b],
            vec![a.clone(), b.clone()],
            JetComputeTapeRule::Matmul,
        );
    }
    let f32_profile = a.last_placement.profile == CPU_ORACLE_F32_PROFILE;
    let mut out = jet_compute_tensor_from_shape_like(a, vec![m, n], 0.0)?;
    for i in 0..m {
        for j in 0..n {
            let mut sum = 0.0;
            for t in 0..k {
                let av = jet_compute_get_raw(a, &vec![i, t])?;
                let bv = jet_compute_get_raw(b, &vec![t, j])?;
                sum = if f32_profile {
                    let av = jet_compute_f32_value(av, "matmul input")?;
                    let bv = jet_compute_f32_value(bv, "matmul input")?;
                    f64::from(av * bv + sum as f32)
                } else {
                    sum + av * bv
                };
                if !sum.is_finite() {
                    return Err(JetComputeError::Arithmetic(
                        "matmul accumulation produced a non-finite value".to_string(),
                    ));
                }
            }
            jet_compute_set(&mut out, &vec![i, j], sum)?;
        }
    }
    jet_compute_record(
        out,
        &[a, b],
        vec![a.clone(), b.clone()],
        JetComputeTapeRule::Matmul,
    )
}

fn jet_compute_device_cpu() -> JetComputeDevice {
    JetComputeDevice::Cpu
}

fn jet_compute_device_auto() -> JetComputeDevice {
    JetComputeDevice::Auto
}

fn jet_compute_device_metal() -> JetComputeDevice {
    JetComputeDevice::Metal
}

fn jet_compute_metal_upload(tensor: &JetTensor) -> Result<(), JetComputeError> {
    if tensor.last_placement.profile != CPU_ORACLE_F32_PROFILE {
        return Err(JetComputeError::Unsupported(
            "Metal transfers support only F32Strict+Reproducible".to_string(),
        ));
    }
    if !jet_compute_metal::available() {
        return Err(JetComputeError::Device(
            "Metal device was lost before transfer".to_string(),
        ));
    }
    let values = jet_compute_tensor_values(tensor)
        .into_iter()
        .map(|value| jet_compute_f32_value(value, "Metal transfer"))
        .collect::<Result<Vec<_>, _>>()?;
    jet_compute_metal::copy(&values).map(|_| ())
}

fn jet_compute_on_device(
    tensor: &JetTensor,
    device: JetComputeDevice,
) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    let mut receipt = jet_compute_place_with_profile(device, &tensor.last_placement.profile)?;
    if receipt.selected == JetComputeDevice::Metal {
        jet_compute_metal_upload(tensor)?;
    }
    if receipt.selected == JetComputeDevice::Cpu
        && tensor.last_placement.profile == CPU_ORACLE_F32_PROFILE
    {
        receipt.reason = if device == JetComputeDevice::Auto {
            "policy=auto; selected=cpu; ability=cpu-oracle.f32".to_string()
        } else {
            "policy=explicit; selected=cpu; ability=cpu-oracle.f32".to_string()
        };
    }
    Ok(JetTensor {
        shape: tensor.shape.clone(),
        strides: tensor.strides.clone(),
        data: tensor.data.clone(),
        device: receipt.selected,
        last_placement: receipt,
        last_transfer: None,
        trace: tensor.trace.clone(),
    })
}

// ── D-COMPUTE1=D / #1136: ndarray broadcast, ufuncs, reductions ─────────────

fn jet_compute_broadcast_shape(
    a: &[i64],
    b: &[i64],
) -> Result<Vec<i64>, JetComputeError> {
    if a.is_empty() || b.is_empty() {
        return Err(JetComputeError::InvalidShape(
            "broadcasting requires ranked tensors".to_string(),
        ));
    }
    if a.iter().chain(b.iter()).any(|dim| *dim < 0) {
        return Err(JetComputeError::InvalidShape(
            "broadcast shapes cannot contain negative axes".to_string(),
        ));
    }
    let rank = a.len().max(b.len());
    let mut out = vec![1i64; rank];
    for i in 0..rank {
        let da = if i < rank - a.len() {
            1
        } else {
            a[i - (rank - a.len())]
        };
        let db = if i < rank - b.len() {
            1
        } else {
            b[i - (rank - b.len())]
        };
        if da == db {
            out[i] = da;
        } else if da == 1 {
            // A singleton axis expands to the other extent, including zero.
            out[i] = db;
        } else if db == 1 {
            // A singleton axis expands to the other extent, including zero.
            out[i] = da;
        } else {
            return Err(JetComputeError::RankMismatch(format!(
                "cannot broadcast shapes {:?} and {:?}",
                a, b
            )));
        }
    }
    jet_compute_storage_len(&out)?;
    Ok(out)
}

fn jet_compute_materialize_broadcast(
    tensor: &JetTensor,
    shape: &[i64],
) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    let n = jet_compute_storage_len(shape)?;
    let strides = jet_compute_row_major_strides(shape)?;
    let src_rank = tensor.shape.len();
    let dst_rank = shape.len();
    if src_rank == 0 || src_rank > dst_rank {
        return Err(JetComputeError::RankMismatch(format!(
            "cannot broadcast rank {} into rank {}",
            src_rank, dst_rank
        )));
    }
    if jet_compute_broadcast_shape(&tensor.shape, shape)? != shape {
        return Err(JetComputeError::InvalidShape(format!(
            "broadcast target {:?} is incompatible with {:?}",
            shape, tensor.shape
        )));
    }
    // Empty output has no source element to read.  This also makes shapes such
    // as [0, 3] broadcast-safe instead of indexing an empty backing vector.
    if n == 0 {
        return jet_compute_tensor_from_shape_like(tensor, shape.to_vec(), 0.0);
    }
    let mut data = Vec::with_capacity(n);
    for flat in 0..n {
        let mut rem = i64::try_from(flat).map_err(|_| {
            JetComputeError::InvalidShape("broadcast index is too large".to_string())
        })?;
        let mut destination_coords = vec![0i64; dst_rank];
        for axis in (0..dst_rank).rev() {
            let dim = shape[axis];
            destination_coords[axis] = if dim == 0 { 0 } else { rem % dim };
            rem = if dim == 0 { 0 } else { rem / dim };
        }
        let rank_delta = dst_rank - src_rank;
        let source_coords = (0..src_rank)
            .map(|axis| {
                if tensor.shape[axis] == 1 {
                    0
                } else {
                    destination_coords[rank_delta + axis]
                }
            })
            .collect::<Vec<_>>();
        data.push(jet_compute_get_raw(tensor, &source_coords)?);
    }
    let mut output = jet_compute_tensor_from_shape_like(tensor, shape.to_vec(), 0.0)?;
    output.strides = strides;
    output.data = std::sync::Arc::new(data);
    Ok(output)
}

fn jet_compute_broadcast_to(
    tensor: &JetTensor,
    shape: &Vec<i64>,
) -> Result<JetTensor, JetComputeError> {
    let out_shape = jet_compute_broadcast_shape(&tensor.shape, shape)?;
    if &out_shape != shape {
        return Err(JetComputeError::InvalidShape(format!(
            "broadcast target {:?} is incompatible with {:?}",
            shape, tensor.shape
        )));
    }
    let output = jet_compute_materialize_broadcast(tensor, shape)?;
    jet_compute_record(
        output,
        &[tensor],
        vec![tensor.clone()],
        JetComputeTapeRule::Broadcast {
            source_shape: tensor.shape.clone(),
        },
    )
}

fn jet_compute_transpose(tensor: &JetTensor) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    if tensor.shape.len() != 2 {
        return Err(JetComputeError::RankMismatch(
            "transpose requires rank-2 tensor".to_string(),
        ));
    }
    let (source_strides, offset) = jet_compute_view_metadata(tensor)?;
    let mut strides = vec![source_strides[1], source_strides[0]];
    if offset != 0 {
        strides.push(i64::try_from(offset).map_err(|_| {
            JetComputeError::InvalidShape("Tensor view offset is too large".to_string())
        })?);
    }
    let out = JetTensor {
        shape: vec![tensor.shape[1], tensor.shape[0]],
        strides,
        data: tensor.data.clone(),
        device: tensor.device,
        last_placement: tensor.last_placement.clone(),
        last_transfer: None,
        trace: None,
    };
    jet_compute_validate_tensor(&out)?;
    jet_compute_record(
        out,
        &[tensor],
        vec![tensor.clone()],
        JetComputeTapeRule::Transpose,
    )
}

fn jet_compute_sum_axis(tensor: &JetTensor, axis: i64) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    let Some(axis) = usize::try_from(axis)
        .ok()
        .filter(|index| *index < tensor.shape.len())
    else {
        return Err(JetComputeError::OutOfBounds(format!(
            "sum_axis axis {} out of range for rank {}",
            axis,
            tensor.shape.len()
        )));
    };
    let mut out_shape = Vec::new();
    for (i, &d) in tensor.shape.iter().enumerate() {
        if i != axis {
            out_shape.push(d);
        }
    }
    if out_shape.is_empty() {
        out_shape.push(1);
    }
    if tensor.device == JetComputeDevice::Metal {
        let mut out = jet_compute_tensor_from_shape_like(tensor, out_shape.clone(), 0.0)?;
        let axis_len = usize::try_from(tensor.shape[axis]).map_err(|_| {
            JetComputeError::InvalidShape("Metal sum_axis extent is too large".to_string())
        })?;
        let out_n = usize::try_from(jet_compute_numel(&out_shape)?).map_err(|_| {
            JetComputeError::InvalidShape("Metal sum_axis output is too large".to_string())
        })?;
        for flat in 0..out_n {
            let mut rem = flat as i64;
            let mut out_coords = vec![0i64; out_shape.len()];
            for index in (0..out_shape.len()).rev() {
                let dim = out_shape[index];
                out_coords[index] = if dim == 0 { 0 } else { rem % dim };
                rem = if dim == 0 { 0 } else { rem / dim };
            }
            let mut coords = vec![0i64; tensor.shape.len()];
            let mut out_index = 0;
            for index in 0..tensor.shape.len() {
                if index != axis {
                    coords[index] = out_coords[out_index];
                    out_index += 1;
                }
            }
            let mut values = Vec::with_capacity(axis_len);
            for value in 0..axis_len {
                coords[axis] = value as i64;
                values.push(jet_compute_f32_value(
                    jet_compute_get_raw(tensor, &coords)?,
                    "Metal sum input",
                )?);
            }
            let sum = if values.is_empty() {
                0.0
            } else {
                jet_compute_metal::sum(&values)?
                    .into_iter()
                    .next()
                    .ok_or_else(|| {
                        JetComputeError::Device("Metal sum returned no value".to_string())
                    })?
            };
            jet_compute_set(&mut out, &out_coords, f64::from(sum))?;
        }
        return jet_compute_record(
            out,
            &[tensor],
            vec![tensor.clone()],
            JetComputeTapeRule::SumAxis {
                axis,
                source_shape: tensor.shape.clone(),
            },
        );
    }
    let f32_profile = tensor.last_placement.profile == CPU_ORACLE_F32_PROFILE;
    let mut out = jet_compute_tensor_from_shape_like(tensor, out_shape.clone(), 0.0)?;
    let axis_len = tensor.shape[axis];
    let out_n = jet_compute_numel(&out_shape)?;
    for flat in 0..out_n {
        let mut coords = vec![0i64; tensor.shape.len()];
        let mut rem = flat;
        let mut out_coords = vec![0i64; out_shape.len()];
        for i in (0..out_shape.len()).rev() {
            let dim = out_shape[i];
            out_coords[i] = if dim == 0 { 0 } else { rem % dim };
            rem = if dim == 0 { 0 } else { rem / dim };
        }
        let mut o = 0usize;
        for i in 0..tensor.shape.len() {
            if i == axis {
                coords[i] = 0;
            } else {
                coords[i] = out_coords[o];
                o += 1;
            }
        }
        let mut sum = 0.0;
        for k in 0..axis_len {
            coords[axis] = k;
            let value = jet_compute_get_raw(tensor, &coords)?;
            sum = if f32_profile {
                let value = jet_compute_f32_value(value, "sum_axis input")?;
                f64::from(value + sum as f32)
            } else {
                sum + value
            };
            if !sum.is_finite() {
                return Err(JetComputeError::Arithmetic(
                    "sum_axis accumulation produced a non-finite value".to_string(),
                ));
            }
        }
        jet_compute_set(&mut out, &out_coords, sum)?;
    }
    jet_compute_record(
        out,
        &[tensor],
        vec![tensor.clone()],
        JetComputeTapeRule::SumAxis {
            axis,
            source_shape: tensor.shape.clone(),
        },
    )
}

fn jet_compute_unary(op: &str, tensor: &JetTensor) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    if !matches!(op, "negate" | "abs" | "exp" | "log" | "sqrt") {
        return Err(JetComputeError::Unsupported(format!(
            "unsupported unary compute operation `{op}`"
        )));
    }
    if tensor.device == JetComputeDevice::Metal {
        let values = jet_compute_metal_values(tensor, "unary input")?;
        if op == "log" && values.iter().any(|value| *value <= 0.0) {
            return Err(JetComputeError::Arithmetic(
                "log requires strictly positive values".to_string(),
            ));
        }
        if op == "sqrt" && values.iter().any(|value| *value < 0.0) {
            return Err(JetComputeError::Arithmetic(
                "sqrt requires non-negative values".to_string(),
            ));
        }
        let data = jet_compute_metal_unary_values(op, &values)?;
        return jet_compute_record(
            jet_compute_metal_result_like(tensor, tensor.shape.clone(), data)?,
            &[tensor],
            vec![tensor.clone()],
            JetComputeTapeRule::Unary(op.to_string()),
        );
    }
    let f32_profile = tensor.last_placement.profile == CPU_ORACLE_F32_PROFILE;
    let values = jet_compute_tensor_values(tensor);
    let mut data = Vec::with_capacity(values.len());
    for value in values {
        let output = if f32_profile {
            let value = jet_compute_f32_value(value, "unary input")?;
            let output = match op {
                "negate" => -value,
                "abs" => value.abs(),
                "exp" => value.exp(),
                "log" if value > 0.0 => value.ln(),
                "log" => {
                    return Err(JetComputeError::Arithmetic(
                        "log requires strictly positive values".to_string(),
                    ));
                }
                "sqrt" if value >= 0.0 => value.sqrt(),
                "sqrt" => {
                    return Err(JetComputeError::Arithmetic(
                        "sqrt requires non-negative values".to_string(),
                    ));
                }
                _ => unreachable!("unvalidated unary operation"),
            };
            f64::from(output)
        } else {
            match op {
                "negate" => -value,
                "abs" => value.abs(),
                "exp" => value.exp(),
                "log" if value > 0.0 => value.ln(),
                "log" => {
                    return Err(JetComputeError::Arithmetic(
                        "log requires strictly positive values".to_string(),
                    ));
                }
                "sqrt" if value >= 0.0 => value.sqrt(),
                "sqrt" => {
                    return Err(JetComputeError::Arithmetic(
                        "sqrt requires non-negative values".to_string(),
                    ));
                }
                _ => unreachable!("unvalidated unary operation"),
            }
        };
        if !output.is_finite() {
            return Err(JetComputeError::Arithmetic(format!(
                "unary operation `{op}` produced a non-finite value"
            )));
        }
        data.push(output);
    }
    let mut output = jet_compute_tensor_from_shape_like(tensor, tensor.shape.clone(), 0.0)?;
    output.strides = jet_compute_row_major_strides(&tensor.shape)?;
    output.data = std::sync::Arc::new(data);
    jet_compute_record(
        output,
        &[tensor],
        vec![tensor.clone()],
        JetComputeTapeRule::Unary(op.to_string()),
    )
}

fn jet_compute_binary(
    op: &str,
    a: &JetTensor,
    b: &JetTensor,
) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(a)?;
    jet_compute_validate_tensor(b)?;
    jet_compute_require_same_contract(a, b, "compute operation")?;
    let shape = jet_compute_broadcast_shape(&a.shape, &b.shape)?;
    if !matches!(op, "sub" | "div" | "maximum" | "minimum" | "add" | "mul") {
        return Err(JetComputeError::Unsupported(format!(
            "unsupported binary compute operation `{op}`"
        )));
    }
    let rule = match op {
        "add" => JetComputeTapeRule::Add,
        "sub" => JetComputeTapeRule::Sub,
        "mul" => JetComputeTapeRule::Mul,
        "div" => JetComputeTapeRule::Div,
        "maximum" => JetComputeTapeRule::Maximum,
        "minimum" => JetComputeTapeRule::Minimum,
        _ => unreachable!("validated binary operation"),
    };
    if a.device == JetComputeDevice::Metal {
        let left = jet_compute_materialize_broadcast(a, &shape)?;
        let right = jet_compute_materialize_broadcast(b, &shape)?;
        let left_values = jet_compute_metal_values(&left, "binary input")?;
        let right_values = jet_compute_metal_values(&right, "binary input")?;
        if op == "div" && right_values.iter().any(|value| *value == 0.0) {
            return Err(JetComputeError::Arithmetic(
                "division by zero in compute operation".to_string(),
            ));
        }
        let data = jet_compute_metal_binary_values(op, &left_values, &right_values)?;
        return jet_compute_record(
            jet_compute_metal_result_like(a, shape, data)?,
            &[a, b],
            vec![a.clone(), b.clone()],
            rule,
        );
    }
    // D-COMPUTE-FUSE1: broadcast indexing and the elementwise operation are
    // one eager Prelude loop. Do not materialize either broadcast operand;
    // this is the shared fusion path for AOT, comptime, and dev evaluation.
    let f32_profile = a.last_placement.profile == CPU_ORACLE_F32_PROFILE;
    let n = jet_compute_storage_len(&shape)?;
    let mut data = Vec::with_capacity(n);
    let rank = shape.len();
    let left_rank_delta = rank - a.shape.len();
    let right_rank_delta = rank - b.shape.len();
    for flat in 0..n {
        let mut rem = i64::try_from(flat).map_err(|_| {
            JetComputeError::InvalidShape("broadcast index is too large".to_string())
        })?;
        let mut output_coords = vec![0i64; rank];
        for axis in (0..rank).rev() {
            let dim = shape[axis];
            output_coords[axis] = if dim == 0 { 0 } else { rem % dim };
            rem = if dim == 0 { 0 } else { rem / dim };
        }
        let left_coords = (0..a.shape.len())
            .map(|axis| {
                if a.shape[axis] == 1 {
                    0
                } else {
                    output_coords[left_rank_delta + axis]
                }
            })
            .collect::<Vec<_>>();
        let right_coords = (0..b.shape.len())
            .map(|axis| {
                if b.shape[axis] == 1 {
                    0
                } else {
                    output_coords[right_rank_delta + axis]
                }
            })
            .collect::<Vec<_>>();
        let x = jet_compute_get_raw(a, &left_coords)?;
        let y = jet_compute_get_raw(b, &right_coords)?;
        if op == "div" && y == 0.0 {
            return Err(JetComputeError::Arithmetic(
                "division by zero in compute operation".to_string(),
            ));
        }
        let output = if f32_profile {
            let x = jet_compute_f32_value(x, "binary operation input")?;
            let y = jet_compute_f32_value(y, "binary operation input")?;
            let output = match op {
                "sub" => x - y,
                "div" => x / y,
                "maximum" => x.max(y),
                "minimum" => x.min(y),
                "add" => x + y,
                "mul" => x * y,
                _ => unreachable!("unvalidated binary operation"),
            };
            f64::from(output)
        } else {
            match op {
                "sub" => x - y,
                "div" => x / y,
                "maximum" => x.max(y),
                "minimum" => x.min(y),
                "add" => x + y,
                "mul" => x * y,
                _ => unreachable!("unvalidated binary operation"),
            }
        };
        if !output.is_finite() {
            return Err(JetComputeError::Arithmetic(
                "compute operation produced a non-finite value".to_string(),
            ));
        }
        data.push(output);
    }
    let strides = jet_compute_row_major_strides(&shape)?;
    let mut output = jet_compute_tensor_from_shape_like(a, shape, 0.0)?;
    output.strides = strides;
    output.data = std::sync::Arc::new(data);
    jet_compute_record(output, &[a, b], vec![a.clone(), b.clone()], rule)
}

// ── #1137 / D-COMPUTE1: dense linalg on the Tensor CPU oracle ───────────────

fn jet_compute_eye(n: i64) -> Result<JetTensor, JetComputeError> {
    if n < 0 {
        return Err(JetComputeError::InvalidShape(
            "eye size must be non-negative".to_string(),
        ));
    }
    let mut out = jet_compute_tensor_from_shape(vec![n, n], 0.0, JetComputeDevice::Cpu)?;
    for i in 0..n {
        jet_compute_set(&mut out, &vec![i, i], 1.0)?;
    }
    Ok(out)
}

fn jet_compute_det(tensor: &JetTensor) -> Result<f64, JetComputeError> {
    if tensor.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "det has no registered autodiff rule".to_string(),
        ));
    }
    if tensor.shape.len() != 2 || tensor.shape[0] != tensor.shape[1] {
        return Err(JetComputeError::RankMismatch(
            "det requires a square rank-2 tensor".to_string(),
        ));
    }
    jet_compute_validate_tensor(tensor)?;
    if tensor.device == JetComputeDevice::Metal {
        return Err(JetComputeError::Unsupported(
            "Metal backend does not support det; transfer to CPU explicitly".to_string(),
        ));
    }
    let n = usize::try_from(tensor.shape[0]).map_err(|_| {
        JetComputeError::InvalidShape("det dimension is too large".to_string())
    })?;
    let matrix_len = n.checked_mul(n).ok_or_else(|| {
        JetComputeError::InvalidShape("det matrix storage length overflow".to_string())
    })?;
    let values = jet_compute_tensor_values(tensor);
    if matrix_len != values.len() {
        return Err(JetComputeError::InvalidShape(
            "det matrix storage is inconsistent".to_string(),
        ));
    }
    let mut a = values.to_vec();
    let mut det = 1.0;
    for i in 0..n {
        let mut pivot = i;
        for r in i..n {
            if a[r * n + i].abs() > a[pivot * n + i].abs() {
                pivot = r;
            }
        }
        if a[pivot * n + i].abs() < 1e-15 {
            return Ok(0.0);
        }
        if pivot != i {
            for c in 0..n {
                a.swap(i * n + c, pivot * n + c);
            }
            det = -det;
        }
        let piv = a[i * n + i];
        det *= piv;
        if !det.is_finite() {
            return Err(JetComputeError::Arithmetic(
                "det overflowed to a non-finite value".to_string(),
            ));
        }
        for r in (i + 1)..n {
            let factor = a[r * n + i] / piv;
            if !factor.is_finite() {
                return Err(JetComputeError::Arithmetic(
                    "det elimination produced a non-finite factor".to_string(),
                ));
            }
            for c in i..n {
                a[r * n + c] -= factor * a[i * n + c];
                if !a[r * n + c].is_finite() {
                    return Err(JetComputeError::Arithmetic(
                        "det elimination produced a non-finite value".to_string(),
                    ));
                }
            }
        }
    }
    Ok(det)
}

fn jet_compute_inv(tensor: &JetTensor) -> Result<JetTensor, JetComputeError> {
    if tensor.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "inv has no registered autodiff rule".to_string(),
        ));
    }
    if tensor.shape.len() != 2 || tensor.shape[0] != tensor.shape[1] {
        return Err(JetComputeError::RankMismatch(
            "inv requires a square rank-2 tensor".to_string(),
        ));
    }
    jet_compute_validate_tensor(tensor)?;
    if tensor.device == JetComputeDevice::Metal {
        return Err(JetComputeError::Unsupported(
            "Metal backend does not support inv; transfer to CPU explicitly".to_string(),
        ));
    }
    let n = usize::try_from(tensor.shape[0]).map_err(|_| {
        JetComputeError::InvalidShape("inv dimension is too large".to_string())
    })?;
    let width = n.checked_mul(2).ok_or_else(|| {
        JetComputeError::InvalidShape("inv augmented width overflow".to_string())
    })?;
    let matrix_len = n.checked_mul(width).ok_or_else(|| {
        JetComputeError::InvalidShape("inv matrix storage length overflow".to_string())
    })?;
    if matrix_len > MAX_TENSOR_ELEMENTS {
        return Err(JetComputeError::InvalidShape(
            "inv workspace exceeds the Core storage limit".to_string(),
        ));
    }
    let mut a = vec![0.0; matrix_len];
    for i in 0..n {
        for j in 0..n {
            a[i * width + j] = jet_compute_get_raw(tensor, &vec![i as i64, j as i64])?;
            a[i * width + n + j] = if i == j { 1.0 } else { 0.0 };
        }
    }
    for i in 0..n {
        let mut pivot = i;
        for r in i..n {
            if a[r * width + i].abs() > a[pivot * width + i].abs() {
                pivot = r;
            }
        }
        if a[pivot * width + i].abs() < 1e-15 {
            return Err(JetComputeError::InvalidShape(
                "matrix is singular".to_string(),
            ));
        }
        if pivot != i {
            for c in 0..width {
                a.swap(i * width + c, pivot * width + c);
            }
        }
        let piv = a[i * width + i];
        for c in 0..width {
            a[i * width + c] /= piv;
            if !a[i * width + c].is_finite() {
                return Err(JetComputeError::Arithmetic(
                    "inv normalization produced a non-finite value".to_string(),
                ));
            }
        }
        for r in 0..n {
            if r == i {
                continue;
            }
            let factor = a[r * width + i];
            for c in 0..width {
                a[r * width + c] -= factor * a[i * width + c];
                if !a[r * width + c].is_finite() {
                    return Err(JetComputeError::Arithmetic(
                        "inv elimination produced a non-finite value".to_string(),
                    ));
                }
            }
        }
    }
    let mut out = jet_compute_tensor_from_shape(
        vec![tensor.shape[0], tensor.shape[1]],
        0.0,
        JetComputeDevice::Cpu,
    )?;
    for i in 0..n {
        for j in 0..n {
            jet_compute_set(&mut out, &vec![i as i64, j as i64], a[i * width + n + j])?;
        }
    }
    Ok(out)
}

fn jet_compute_solve(a: &JetTensor, b: &JetTensor) -> Result<JetTensor, JetComputeError> {
    if a.trace.is_some() || b.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "solve has no registered autodiff rule".to_string(),
        ));
    }
    if a.shape.len() != 2 || a.shape[0] != a.shape[1] {
        return Err(JetComputeError::RankMismatch(
            "solve requires a square rank-2 coefficient tensor".to_string(),
        ));
    }
    jet_compute_validate_tensor(a)?;
    jet_compute_validate_tensor(b)?;
    if a.device == JetComputeDevice::Metal || b.device == JetComputeDevice::Metal {
        return Err(JetComputeError::Unsupported(
            "Metal backend does not support solve; transfer inputs to CPU explicitly".to_string(),
        ));
    }
    let n = usize::try_from(a.shape[0])
        .map_err(|_| JetComputeError::InvalidShape("solve dimension is too large".to_string()))?;
    let rhs_cols = match b.shape.as_slice() {
        [rows] if *rows == a.shape[0] => 1,
        [rows, cols] if *rows == a.shape[0] && *cols >= 0 => usize::try_from(*cols).map_err(|_| {
            JetComputeError::InvalidShape("solve right-hand side is too large".to_string())
        })?,
        _ => {
            return Err(JetComputeError::RankMismatch(format!(
                "solve expects a length-{} vector or a matrix with {} rows",
                a.shape[0], a.shape[0]
            )))
        }
    };
    let width = n.checked_add(rhs_cols).ok_or_else(|| {
        JetComputeError::InvalidShape("solve augmented width overflow".to_string())
    })?;
    let workspace = n.checked_mul(width).ok_or_else(|| {
        JetComputeError::InvalidShape("solve workspace length overflow".to_string())
    })?;
    if workspace > MAX_TENSOR_ELEMENTS {
        return Err(JetComputeError::InvalidShape(
            "solve workspace exceeds the Core storage limit".to_string(),
        ));
    }
    let mut augmented = vec![vec![0.0; width]; n];
    for row in 0..n {
        for col in 0..n {
            augmented[row][col] = jet_compute_get_raw(a, &[row as i64, col as i64])?;
        }
        for col in 0..rhs_cols {
            augmented[row][n + col] = if b.shape.len() == 1 {
                jet_compute_get_raw(b, &[row as i64])?
            } else {
                jet_compute_get_raw(b, &[row as i64, col as i64])?
            };
        }
    }
    for pivot in 0..n {
        let mut best = pivot;
        for row in pivot..n {
            if augmented[row][pivot].abs() > augmented[best][pivot].abs() {
                best = row;
            }
        }
        if augmented[best][pivot].abs() < 1e-15 {
            return Err(JetComputeError::Arithmetic(
                "solve coefficient matrix is singular".to_string(),
            ));
        }
        augmented.swap(pivot, best);
        let divisor = augmented[pivot][pivot];
        for value in &mut augmented[pivot][pivot..] {
            *value /= divisor;
            if !value.is_finite() {
                return Err(JetComputeError::Arithmetic(
                    "solve normalization produced a non-finite value".to_string(),
                ));
            }
        }
        for row in 0..n {
            if row == pivot {
                continue;
            }
            let factor = augmented[row][pivot];
            if !factor.is_finite() {
                return Err(JetComputeError::Arithmetic(
                    "solve elimination produced a non-finite factor".to_string(),
                ));
            }
            for col in pivot..width {
                augmented[row][col] -= factor * augmented[pivot][col];
                if !augmented[row][col].is_finite() {
                    return Err(JetComputeError::Arithmetic(
                        "solve elimination produced a non-finite value".to_string(),
                    ));
                }
            }
        }
    }
    let output_shape = if b.shape.len() == 1 {
        vec![a.shape[0]]
    } else {
        vec![a.shape[0], b.shape[1]]
    };
    let mut out = jet_compute_tensor_from_shape(output_shape, 0.0, JetComputeDevice::Cpu)?;
    for row in 0..n {
        for col in 0..rhs_cols {
            let index = if b.shape.len() == 1 {
                vec![row as i64]
            } else {
                vec![row as i64, col as i64]
            };
            jet_compute_set(&mut out, &index, augmented[row][n + col])?;
        }
    }
    Ok(out)
}

/// Naive DFT on a rank-1 real tensor → interleaved [re, im, re, im, …] length 2n.
fn jet_compute_fft(tensor: &JetTensor) -> Result<JetTensor, JetComputeError> {
    if tensor.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "fft has no registered autodiff rule".to_string(),
        ));
    }
    jet_compute_validate_tensor(tensor)?;
    if tensor.device == JetComputeDevice::Metal {
        return Err(JetComputeError::Unsupported(
            "Metal backend does not support fft; transfer to CPU explicitly".to_string(),
        ));
    }
    if tensor.shape.len() != 1 {
        return Err(JetComputeError::RankMismatch(
            "fft requires a rank-1 tensor".to_string(),
        ));
    }
    let values = jet_compute_tensor_values(tensor);
    let n = values.len();
    let output_len = n
        .checked_mul(2)
        .and_then(|length| i64::try_from(length).ok())
        .ok_or_else(|| JetComputeError::InvalidShape("fft output length overflow".to_string()))?;
    let mut out = jet_compute_tensor_from_shape(
        vec![output_len],
        0.0,
        JetComputeDevice::Cpu,
    )?;
    if n == 0 {
        return Ok(out);
    }
    for k in 0..n {
        let mut re = 0.0;
        let mut im = 0.0;
        for t in 0..n {
            let angle = -2.0 * std::f64::consts::PI * (k as f64) * (t as f64) / (n as f64);
            re += values[t] * angle.cos();
            im += values[t] * angle.sin();
        }
        jet_compute_set(&mut out, &vec![(2 * k) as i64], re)?;
        jet_compute_set(&mut out, &vec![(2 * k + 1) as i64], im)?;
    }
    Ok(out)
}

// ── #1138 / #1145: stream + transfer receipts ───────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetComputeStream {
    id: i64,
    device: JetComputeDevice,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct JetComputeTransferReceipt {
    from: JetComputeDevice,
    to: JetComputeDevice,
    bytes: i64,
    fallback: String,
}

fn jet_compute_validate_transfer_receipt(
    tensor: &JetTensor,
    receipt: &JetComputeTransferReceipt,
) -> Result<(), JetComputeError> {
    if receipt.from == JetComputeDevice::Auto || receipt.to == JetComputeDevice::Auto {
        return Err(JetComputeError::Device(
            "transfer receipt must name concrete source and destination devices".to_string(),
        ));
    }
    if receipt.to != tensor.device {
        return Err(JetComputeError::Device(
            "transfer receipt destination does not match Tensor placement".to_string(),
        ));
    }
    let scalar_bytes = if tensor.last_placement.profile == CPU_ORACLE_F32_PROFILE {
        std::mem::size_of::<f32>()
    } else {
        std::mem::size_of::<f64>()
    };
    let logical_bytes = jet_compute_tensor_values(tensor)
        .len()
        .checked_mul(scalar_bytes)
        .and_then(|bytes| i64::try_from(bytes).ok())
        .ok_or_else(|| JetComputeError::Device("transfer byte count overflow".to_string()))?;
    let expected_bytes = if receipt.from == receipt.to {
        0
    } else {
        logical_bytes
    };
    if receipt.bytes != expected_bytes
        || (receipt.from == receipt.to && receipt.fallback != "none")
        || (receipt.from != receipt.to && receipt.fallback == "none")
    {
        return Err(JetComputeError::Device(
            "transfer receipt does not match the selected backend operation".to_string(),
        ));
    }
    Ok(())
}

impl JetShow for JetComputeStream {
    fn jet_show(&self) -> String {
        // Stream identity is runtime-local; exposing it would make AOT/JIT
        // output differ despite identical compute semantics.
        format!("ComputeStream(device={})", self.device.jet_show())
    }
}

impl JetShow for JetComputeTransferReceipt {
    fn jet_show(&self) -> String {
        format!(
            "Transfer(from={}, to={}, bytes={}, fallback={})",
            self.from.jet_show(),
            self.to.jet_show(),
            self.bytes,
            self.fallback
        )
    }
}

fn jet_compute_stream_new() -> JetComputeStream {
    jet_compute_stream_new_on_device(JetComputeDevice::Cpu)
        .unwrap_or_else(|error| jet_panic("Compute.rs", line!(), &error.jet_show()))
}

fn jet_compute_stream_new_on_device(
    requested: JetComputeDevice,
) -> Result<JetComputeStream, JetComputeError> {
    let receipt = jet_compute_place_with_profile(requested, CPU_ORACLE_F32_PROFILE)?;
    static NEXT_STREAM_ID: std::sync::atomic::AtomicI64 =
        std::sync::atomic::AtomicI64::new(1);
    Ok(JetComputeStream {
        id: NEXT_STREAM_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        device: receipt.selected,
    })
}

fn jet_compute_stream_sync(stream: &JetComputeStream) -> Result<(), JetComputeError> {
    if stream.id <= 0 {
        return Err(JetComputeError::Device(
            "cannot synchronize an invalid compute stream".to_string(),
        ));
    }
    if stream.device == JetComputeDevice::Metal && !jet_compute_metal::available() {
        return Err(JetComputeError::Device(
            "Metal device was lost before stream synchronization".to_string(),
        ));
    }
    Ok(())
}

fn jet_compute_stream_show(stream: &JetComputeStream) -> String {
    stream.jet_show()
}

fn jet_compute_transfer(
    tensor: &JetTensor,
    device: JetComputeDevice,
) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    let scalar_bytes = if tensor.last_placement.profile == CPU_ORACLE_F32_PROFILE {
        std::mem::size_of::<f32>()
    } else {
        std::mem::size_of::<f64>()
    };
    let logical_byte_count = jet_compute_tensor_values(tensor)
        .len()
        .checked_mul(scalar_bytes)
        .and_then(|bytes| i64::try_from(bytes).ok())
        .ok_or_else(|| JetComputeError::Device("transfer byte count overflow".to_string()))?;
    let from = tensor.device;
    let mut out = jet_compute_on_device(tensor, device)?;
    if from == JetComputeDevice::Metal && out.device == JetComputeDevice::Cpu {
        let values = jet_compute_metal_values(tensor, "download")?;
        let values = jet_compute_metal::copy(&values)?;
        out.data = std::sync::Arc::new(values.into_iter().map(f64::from).collect());
        jet_compute_validate_tensor(&out)?;
    }
    let (bytes, transfer_kind) = if from == out.device {
        (0, "no-op; same backend and allocation".to_string())
    } else {
        (logical_byte_count, "copy; selected backend".to_string())
    };
    out.last_placement.reason = format!(
        "transfer kind={transfer_kind} bytes={bytes} from={} to={}",
        from.jet_show(),
        out.device.jet_show()
    );
    out.last_transfer = Some(JetComputeTransferReceipt {
        from,
        to: out.device,
        bytes,
        fallback: if from == out.device {
            "none".to_string()
        } else {
            "not-applicable".to_string()
        },
    });
    Ok(out)
}

fn jet_compute_transfer_show(tensor: &JetTensor) -> String {
    tensor
        .last_transfer
        .as_ref()
        .map_or_else(|| tensor.last_placement.jet_show(), |receipt| receipt.jet_show())
}

// ── #1139 / #1140: safe kernel bounds + typed raw-kernel boundary ────────────

fn jet_compute_kernel_bounds_ok(
    shape: &[i64],
    indices: &[i64],
) -> Result<bool, JetComputeError> {
    if shape.len() != indices.len() {
        return Err(JetComputeError::RankMismatch(
            "kernel index rank must match tensor shape".to_string(),
        ));
    }
    jet_compute_storage_len(shape)?;
    if shape.iter().any(|dim| *dim < 0) {
        return Err(JetComputeError::InvalidShape(
            "kernel shape axes must be non-negative".to_string(),
        ));
    }
    for (i, (&idx, &dim)) in indices.iter().zip(shape.iter()).enumerate() {
        if idx < 0 || idx >= dim {
            return Err(JetComputeError::OutOfBounds(format!(
                "kernel index {idx} out of bounds for axis {i} (extent {dim})"
            )));
        }
    }
    Ok(true)
}

// ── #1141 / D-COMPUTE-AUTODIFF1: reverse-mode VJP + JVP for dense ops ────────

/// Reverse-mode broadcast rule: axes introduced by broadcasting and axes with
/// extent one are summed back into the operand's original shape.
fn jet_compute_reduce_to_shape(
    tensor: &JetTensor,
    target_shape: &[i64],
) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    if target_shape.is_empty() || target_shape.len() > tensor.shape.len() {
        return Err(JetComputeError::RankMismatch(
            "gradient target must be a ranked tensor with no greater rank".to_string(),
        ));
    }
    if jet_compute_broadcast_shape(target_shape, &tensor.shape)? != tensor.shape {
        return Err(JetComputeError::RankMismatch(format!(
            "gradient shape {:?} is not broadcast-compatible with {:?}",
            target_shape, tensor.shape
        )));
    }
    let f32_profile = tensor.last_placement.profile == CPU_ORACLE_F32_PROFILE;
    let mut out = jet_compute_tensor_from_shape_like(tensor, target_shape.to_vec(), 0.0)?;
    let rank_delta = tensor.shape.len() - target_shape.len();
    let values = jet_compute_tensor_values(tensor);
    if tensor.device == JetComputeDevice::Metal {
        let output_len = jet_compute_storage_len(target_shape)?;
        let mut buckets = vec![Vec::<f32>::new(); output_len];
        for (flat, value) in values.iter().enumerate() {
            let mut rem = flat as i64;
            let mut output_coords = vec![0i64; tensor.shape.len()];
            for axis in (0..tensor.shape.len()).rev() {
                let dim = tensor.shape[axis];
                output_coords[axis] = if dim == 0 { 0 } else { rem % dim };
                rem = if dim == 0 { 0 } else { rem / dim };
            }
            let mut target_coords = vec![0i64; target_shape.len()];
            for axis in 0..target_shape.len() {
                let source_axis = axis + rank_delta;
                target_coords[axis] = if target_shape[axis] == 1 {
                    0
                } else {
                    output_coords[source_axis]
                };
            }
            let target_offset = jet_compute_offset(&out, &target_coords)?;
            buckets[target_offset].push(jet_compute_f32_value(
                *value,
                "Metal gradient accumulation",
            )?);
        }
        let mut sums = Vec::with_capacity(output_len);
        for bucket in buckets {
            let sum = if bucket.is_empty() {
                0.0
            } else {
                jet_compute_metal::sum(&bucket)?
                    .into_iter()
                    .next()
                    .ok_or_else(|| {
                        JetComputeError::Device(
                            "Metal gradient reduction returned no value".to_string(),
                        )
                    })?
            };
            sums.push(sum);
        }
        let Some(storage) = std::sync::Arc::get_mut(&mut out.data) else {
            return Err(JetComputeError::Unsupported(
                "Metal gradient accumulation requires exclusive output storage".to_string(),
            ));
        };
        for (slot, sum) in storage.iter_mut().zip(sums) {
            *slot = f64::from(sum);
        }
        return jet_compute_record(
            out,
            &[tensor],
            vec![tensor.clone()],
            JetComputeTapeRule::ReduceToShape {
                source_shape: tensor.shape.clone(),
            },
        );
    }
    for flat in 0..values.len() {
        let mut rem = flat as i64;
        let mut output_coords = vec![0i64; tensor.shape.len()];
        for axis in (0..tensor.shape.len()).rev() {
            let dim = tensor.shape[axis];
            output_coords[axis] = if dim == 0 { 0 } else { rem % dim };
            rem = if dim == 0 { 0 } else { rem / dim };
        }
        let mut target_coords = vec![0i64; target_shape.len()];
        for axis in 0..target_shape.len() {
            let source_axis = axis + rank_delta;
            target_coords[axis] = if target_shape[axis] == 1 {
                0
            } else {
                output_coords[source_axis]
            };
        }
        let target_offset = jet_compute_offset(&out, &target_coords)?;
        let Some(data) = std::sync::Arc::get_mut(&mut out.data) else {
            return Err(JetComputeError::Unsupported(
                "gradient accumulation requires exclusive output storage".to_string(),
            ));
        };
        let Some(slot) = data.get_mut(target_offset) else {
            return Err(JetComputeError::OutOfBounds(
                "gradient accumulation is outside storage".to_string(),
            ));
        };
        *slot = if f32_profile {
            let previous = jet_compute_f32_value(*slot, "gradient accumulation")?;
            let value = jet_compute_f32_value(values[flat], "gradient accumulation")?;
            f64::from(previous + value)
        } else {
            *slot + values[flat]
        };
        if !slot.is_finite() {
            return Err(JetComputeError::Arithmetic(
                "gradient accumulation produced a non-finite value".to_string(),
            ));
        }
    }
    jet_compute_record(
        out,
        &[tensor],
        vec![tensor.clone()],
        JetComputeTapeRule::ReduceToShape {
            source_shape: tensor.shape.clone(),
        },
    )
}

fn jet_compute_zero_like(tensor: &JetTensor) -> Result<JetTensor, JetComputeError> {
    Ok(jet_compute_inherit_placement(
        jet_compute_tensor_from_shape(tensor.shape.clone(), 0.0, JetComputeDevice::Cpu)?,
        tensor,
    ))
}

fn jet_compute_tensor_from_values_like(
    template: &JetTensor,
    values: &[f64],
) -> Result<JetTensor, JetComputeError> {
    let expected = jet_compute_storage_len(&template.shape)?;
    if values.len() != expected || values.iter().any(|value| !value.is_finite()) {
        return Err(JetComputeError::Arithmetic(
            "autodiff values do not match the Tensor shape".to_string(),
        ));
    }
    let mut output = jet_compute_tensor_from_shape(
        template.shape.clone(),
        0.0,
        JetComputeDevice::Cpu,
    )?;
    let Some(storage) = std::sync::Arc::get_mut(&mut output.data) else {
        return Err(JetComputeError::Unsupported(
            "autodiff output requires exclusive storage".to_string(),
        ));
    };
    storage.clone_from_slice(values);
    let output = jet_compute_inherit_placement(output, template);
    jet_compute_validate_tensor(&output)?;
    Ok(output)
}

fn jet_compute_unary_vjp(
    op: &str,
    input: &JetTensor,
    output: &JetTensor,
    cot: &JetTensor,
) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(input)?;
    jet_compute_validate_tensor(output)?;
    jet_compute_validate_tensor(cot)?;
    if input.shape != output.shape || output.shape != cot.shape {
        return Err(JetComputeError::RankMismatch(
            "unary cotangent shape must equal the unary output".to_string(),
        ));
    }
    jet_compute_require_same_contract(input, output, "unary cotangent")?;
    jet_compute_require_same_contract(output, cot, "unary cotangent")?;
    let input_values = jet_compute_tensor_values(input);
    match op {
        "negate" => jet_compute_unary("negate", cot),
        "abs" => {
            let signs = input_values
                .iter()
                .map(|value| {
                    if *value > 0.0 {
                        1.0
                    } else if *value < 0.0 {
                        -1.0
                    } else {
                        0.0
                    }
                })
                .collect::<Vec<_>>();
            let signs = jet_compute_tensor_from_values_like(input, &signs)?;
            jet_compute_binary("mul", &signs, cot)
        }
        "exp" => jet_compute_binary("mul", output, cot),
        "log" => jet_compute_binary("div", cot, input),
        "sqrt" => {
            let two = jet_compute_full(&output.shape, 2.0)?;
            let denominator = jet_compute_binary("mul", &two, output)?;
            jet_compute_binary("div", cot, &denominator)
        }
        _ => Err(JetComputeError::Unsupported(format!(
            "unsupported unary derivative `{op}`"
        ))),
    }
}

fn jet_compute_rule_gradients(
    rule: &JetComputeTapeRule,
    values: &[JetTensor],
    output: &JetTensor,
    cot: &JetTensor,
    active_tape: &std::sync::Arc<std::sync::Mutex<JetComputeTape>>,
) -> Result<Vec<JetTensor>, JetComputeError> {
    let cot = jet_compute_remove_trace_level(cot, active_tape);
    match rule {
        JetComputeTapeRule::Add => {
            let a = jet_compute_reduce_to_shape(&cot, &values[0].shape)?;
            let b = jet_compute_reduce_to_shape(&cot, &values[1].shape)?;
            Ok(vec![
                jet_compute_inherit_placement(a, &values[0]),
                jet_compute_inherit_placement(b, &values[1]),
            ])
        }
        JetComputeTapeRule::Sub => {
            let a = jet_compute_reduce_to_shape(&cot, &values[0].shape)?;
            let negative = jet_compute_unary("negate", &cot)?;
            let b = jet_compute_reduce_to_shape(&negative, &values[1].shape)?;
            Ok(vec![
                jet_compute_inherit_placement(a, &values[0]),
                jet_compute_inherit_placement(b, &values[1]),
            ])
        }
        JetComputeTapeRule::Mul => {
            let a_full = jet_compute_binary("mul", &values[1], &cot)?;
            let b_full = jet_compute_binary("mul", &values[0], &cot)?;
            let a = jet_compute_reduce_to_shape(&a_full, &values[0].shape)?;
            let b = jet_compute_reduce_to_shape(&b_full, &values[1].shape)?;
            Ok(vec![
                jet_compute_inherit_placement(a, &values[0]),
                jet_compute_inherit_placement(b, &values[1]),
            ])
        }
        JetComputeTapeRule::Div => {
            let a_full = jet_compute_binary("div", &cot, &values[1])?;
            let denominator = jet_compute_binary("mul", &values[1], &values[1])?;
            let numerator = jet_compute_binary("mul", &values[0], &cot)?;
            let b_full = jet_compute_unary(
                "negate",
                &jet_compute_binary("div", &numerator, &denominator)?,
            )?;
            let a = jet_compute_reduce_to_shape(&a_full, &values[0].shape)?;
            let b = jet_compute_reduce_to_shape(&b_full, &values[1].shape)?;
            Ok(vec![
                jet_compute_inherit_placement(a, &values[0]),
                jet_compute_inherit_placement(b, &values[1]),
            ])
        }
        JetComputeTapeRule::Maximum | JetComputeTapeRule::Minimum => {
            let maximum = matches!(rule, JetComputeTapeRule::Maximum);
            let output_values = jet_compute_tensor_values(output);
            let left_value = if values[0].shape == output.shape {
                values[0].clone()
            } else {
                jet_compute_materialize_broadcast(&values[0], &output.shape)?
            };
            let right_value = if values[1].shape == output.shape {
                values[1].clone()
            } else {
                jet_compute_materialize_broadcast(&values[1], &output.shape)?
            };
            let left_values = jet_compute_tensor_values(&left_value);
            let right_values = jet_compute_tensor_values(&right_value);
            let mut left_mask = Vec::with_capacity(output_values.len());
            let mut right_mask = Vec::with_capacity(output_values.len());
            for ((output, a), b) in output_values
                .iter()
                .zip(left_values.iter())
                .zip(right_values.iter())
            {
                if *a == *b {
                    return Err(JetComputeError::Unsupported(
                        "maximum/minimum has no derivative at a tie".to_string(),
                    ));
                }
                let left_slot = if (maximum && *a == *output) || (!maximum && *a == *output) {
                    1.0
                } else {
                    0.0
                };
                let right_slot = if (maximum && *b == *output) || (!maximum && *b == *output) {
                    1.0
                } else {
                    0.0
                };
                left_mask.push(left_slot);
                right_mask.push(right_slot);
            }
            let left_mask = jet_compute_tensor_from_values_like(output, &left_mask)?;
            let right_mask = jet_compute_tensor_from_values_like(output, &right_mask)?;
            let left = jet_compute_binary("mul", &left_mask, &cot)?;
            let right = jet_compute_binary("mul", &right_mask, &cot)?;
            Ok(vec![
                jet_compute_reduce_to_shape(&left, &values[0].shape)?,
                jet_compute_reduce_to_shape(&right, &values[1].shape)?,
            ])
        }
        JetComputeTapeRule::Matmul => {
            let (a, b) = jet_compute_vjp_matmul(&values[0], &values[1], &cot)?;
            Ok(vec![a, b])
        }
        JetComputeTapeRule::MatmulF32Tile => {
            let (a, b) = jet_compute_vjp_matmul_f32_tile(&values[0], &values[1], &cot)?;
            Ok(vec![a, b])
        }
        JetComputeTapeRule::MseLoss => Ok(vec![
            jet_compute_mse_vjp(&values[0], &values[1], &cot, true)?,
            jet_compute_mse_vjp(&values[0], &values[1], &cot, false)?,
        ]),
        JetComputeTapeRule::SgdStep { learning_rate } => {
            let (parameter, gradient) = jet_compute_sgd_vjp(
                &values[0],
                &values[1],
                &cot,
                *learning_rate,
            )?;
            Ok(vec![parameter, gradient])
        }
        JetComputeTapeRule::Unary(op) => Ok(vec![jet_compute_unary_vjp(
            op,
            &values[0],
            output,
            &cot,
        )?]),
        JetComputeTapeRule::Reshape { source_shape } => Ok(vec![jet_compute_reshape(
            &cot,
            &source_shape.clone(),
        )?]),
        JetComputeTapeRule::Broadcast { source_shape } => Ok(vec![
            jet_compute_reduce_to_shape(&cot, source_shape)?,
        ]),
        JetComputeTapeRule::ReduceToShape { source_shape } => Ok(vec![
            jet_compute_broadcast_to(&cot, source_shape)?,
        ]),
        JetComputeTapeRule::Transpose => Ok(vec![jet_compute_transpose(&cot)?]),
        JetComputeTapeRule::SumAxis { axis, source_shape } => {
            let mut reduced_shape = source_shape.clone();
            reduced_shape[*axis] = 1;
            let cot = jet_compute_reshape(&cot, &reduced_shape)?;
            Ok(vec![jet_compute_broadcast_to(&cot, source_shape)?])
        }
    }
}

fn jet_compute_reverse(
    state: &JetComputeVjpState,
    seed: &JetTensor,
) -> Result<Vec<JetTensor>, JetComputeError> {
    jet_compute_validate_tensor(&state.value)?;
    jet_compute_validate_tensor(seed)?;
    if state.value.shape != seed.shape {
        return Err(JetComputeError::RankMismatch(
            "VJP seed shape must equal the function output shape".to_string(),
        ));
    }
    jet_compute_require_same_contract(&state.value, seed, "VJP seed")?;
    let (nodes, inputs) = {
        let tape = state
            .tape
            .lock()
            .map_err(|_| JetComputeError::Unsupported("autodiff tape is poisoned".to_string()))?;
        (tape.nodes.clone(), tape.inputs.clone())
    };
    let mut cotangents: Vec<Option<JetTensor>> = vec![None; nodes.len()];
    if let Some(output_node) = state.output_node {
        let Some(slot) = cotangents.get_mut(output_node) else {
            return Err(JetComputeError::Unsupported(
                "VJP output node is outside its tape".to_string(),
            ));
        };
        *slot = Some(jet_compute_untracked(seed));
    }
    // The input nodes are leaves. Keep their accumulated cotangents in place
    // for the final result; only reverse-propagate through operation nodes.
    for index in (inputs.len()..nodes.len()).rev() {
        let Some(cot) = cotangents[index].take() else {
            continue;
        };
        let node = &nodes[index];
        let Some(rule) = &node.rule else {
            continue;
        };
        let gradients = jet_compute_rule_gradients(
            rule,
            &node.values,
            &node.output,
            &cot,
            &state.tape,
        )?;
        for (parent, gradient) in node.parents.iter().zip(gradients) {
            let Some(parent) = parent else {
                continue;
            };
            let gradient = jet_compute_remove_trace_level(&gradient, &state.tape);
            let Some(slot) = cotangents.get_mut(*parent) else {
                return Err(JetComputeError::Unsupported(
                    "VJP parent node is outside its tape".to_string(),
                ));
            };
            *slot = Some(match slot.take() {
                Some(previous) => jet_compute_binary("add", &previous, &gradient)?,
                None => gradient,
            });
        }
    }
    let mut result = Vec::with_capacity(inputs.len());
    for (index, input) in inputs.iter().enumerate() {
        let gradient = cotangents
            .get(index)
            .and_then(Option::as_ref)
            .cloned()
            .unwrap_or(jet_compute_zero_like(input)?);
        result.push(jet_compute_inherit_placement(gradient, input));
    }
    Ok(result)
}

fn jet_compute_select_gradients(
    all: Vec<JetTensor>,
    targets: &[i64],
) -> Result<Vec<JetTensor>, JetComputeError> {
    targets
        .iter()
        .map(|target| {
            let index = usize::try_from(*target).map_err(|_| {
                JetComputeError::Unsupported("negative autodiff target index".to_string())
            })?;
            all.get(index).cloned().ok_or_else(|| {
                JetComputeError::Unsupported("autodiff target index is outside the function signature".to_string())
            })
        })
        .collect()
}

fn jet_compute_gradient_seed(state: &JetComputeVjpState) -> Result<JetTensor, JetComputeError> {
    if jet_compute_tensor_numel(&state.value) != 1 {
        return Err(JetComputeError::RankMismatch(
            "compute.gradient requires a scalar Tensor output".to_string(),
        ));
    }
    Ok(jet_compute_inherit_placement(
        jet_compute_ones(&state.value.shape)?,
        &state.value,
    ))
}

fn jet_compute_vjp_pull(
    state: &JetComputeVjpState,
    seed: &JetTensor,
    targets: &[i64],
) -> Result<Vec<JetTensor>, JetComputeError> {
    jet_compute_select_gradients(jet_compute_reverse(state, seed)?, targets)
}

fn jet_compute_vjp_pull_or_panic(
    state: &JetComputeVjpState,
    seed: &JetTensor,
    targets: &[i64],
    context: &str,
) -> Vec<JetTensor> {
    match jet_compute_vjp_pull(state, seed, targets) {
        Ok(values) => values,
        Err(error) => jet_panic("Compute.rs", line!(), &format!("{context}: {}", error.jet_show())),
    }
}

fn jet_compute_gradient_or_panic(
    state: &JetComputeVjpState,
    targets: &[i64],
    context: &str,
) -> Vec<JetTensor> {
    let seed = match jet_compute_gradient_seed(state) {
        Ok(seed) => seed,
        Err(error) => jet_panic("Compute.rs", line!(), &format!("{context}: {}", error.jet_show())),
    };
    jet_compute_vjp_pull_or_panic(state, &seed, targets, context)
}

fn jet_compute_vjp_unit_grads_or_panic(
    state: &JetComputeVjpState,
    targets: &[i64],
    context: &str,
) -> Vec<JetTensor> {
    jet_compute_gradient_or_panic(state, targets, context)
}

/// The one transform dispatcher used by AOT and the interpreter.  Engines
/// marshal callable arguments and package the typed result; this function
/// owns transform selection, scalar seeding, value detachment, and lazy VJP
/// state creation.
fn jet_compute_transform(
    method: &str,
    state: &JetComputeVjpState,
    tangents: &[JetTensor],
    targets: &[i64],
) -> Result<JetComputeTransformResult, JetComputeError> {
    let value = jet_compute_remove_trace_level(&state.value, &state.tape);
    match method {
        "gradient" => Ok(JetComputeTransformResult::Gradient(
            jet_compute_vjp_pull(state, &jet_compute_gradient_seed(state)?, targets)?,
        )),
        "value_and_gradient" => Ok(JetComputeTransformResult::ValueAndGradient {
            value,
            gradients: jet_compute_vjp_pull(state, &jet_compute_gradient_seed(state)?, targets)?,
        }),
        "vjp" => Ok(JetComputeTransformResult::Vjp {
            value,
            state: state.clone(),
        }),
        "jvp" => Ok(JetComputeTransformResult::Jvp {
            value,
            tangent: jet_compute_jvp(state, tangents.to_vec())?,
        }),
        _ => Err(JetComputeError::Unsupported(format!(
            "unknown autodiff transform `{method}`"
        ))),
    }
}

fn jet_compute_transform_or_panic(
    method: &str,
    state: &JetComputeVjpState,
    tangents: &[JetTensor],
    targets: &[i64],
    context: &str,
) -> JetComputeTransformResult {
    match jet_compute_transform(method, state, tangents, targets) {
        Ok(result) => result,
        Err(error) => jet_panic("Compute.rs", line!(), &format!("{context}: {}", error.jet_show())),
    }
}

fn jet_compute_nested_gradient(
    states: &[JetComputeVjpState],
    targets: &[i64],
) -> Result<Vec<Vec<JetTensor>>, JetComputeError> {
    states
        .iter()
        .map(|state| {
            let result = jet_compute_transform("gradient", state, &[], targets)?;
            let JetComputeTransformResult::Gradient(values) = result else {
                return Err(JetComputeError::Unsupported(
                    "nested gradient did not return gradients".to_string(),
                ));
            };
            Ok(values)
        })
        .collect()
}

fn jet_compute_nested_gradient_or_panic(
    states: &[JetComputeVjpState],
    targets: &[i64],
    context: &str,
) -> Vec<Vec<JetTensor>> {
    match jet_compute_nested_gradient(states, targets) {
        Ok(values) => values,
        Err(error) => jet_panic("Compute.rs", line!(), &format!("{context}: {}", error.jet_show())),
    }
}

fn jet_compute_jvp_rule(
    rule: &JetComputeTapeRule,
    values: &[JetTensor],
    output: &JetTensor,
    tangents: &[JetTensor],
) -> Result<JetTensor, JetComputeError> {
    match rule {
        JetComputeTapeRule::Add => jet_compute_binary("add", &tangents[0], &tangents[1]),
        JetComputeTapeRule::Sub => jet_compute_binary("sub", &tangents[0], &tangents[1]),
        JetComputeTapeRule::Mul => {
            let left = jet_compute_binary("mul", &tangents[0], &values[1])?;
            let right = jet_compute_binary("mul", &values[0], &tangents[1])?;
            jet_compute_binary("add", &left, &right)
        }
        JetComputeTapeRule::Div => {
            let left = jet_compute_binary("div", &tangents[0], &values[1])?;
            let numerator = jet_compute_binary("mul", &values[0], &tangents[1])?;
            let denominator = jet_compute_binary("mul", &values[1], &values[1])?;
            let right = jet_compute_binary("div", &numerator, &denominator)?;
            jet_compute_binary("sub", &left, &right)
        }
        JetComputeTapeRule::Maximum | JetComputeTapeRule::Minimum => {
            let maximum = matches!(rule, JetComputeTapeRule::Maximum);
            let output_values = jet_compute_tensor_values(output);
            let left_value = if values[0].shape == output.shape {
                values[0].clone()
            } else {
                jet_compute_materialize_broadcast(&values[0], &output.shape)?
            };
            let right_value = if values[1].shape == output.shape {
                values[1].clone()
            } else {
                jet_compute_materialize_broadcast(&values[1], &output.shape)?
            };
            let left_tangent = if tangents[0].shape == output.shape {
                tangents[0].clone()
            } else {
                jet_compute_broadcast_to(&tangents[0], &output.shape.to_vec())?
            };
            let right_tangent = if tangents[1].shape == output.shape {
                tangents[1].clone()
            } else {
                jet_compute_broadcast_to(&tangents[1], &output.shape.to_vec())?
            };
            let left_values = jet_compute_tensor_values(&left_value);
            let right_values = jet_compute_tensor_values(&right_value);
            let mut left_mask = Vec::with_capacity(output_values.len());
            let mut right_mask = Vec::with_capacity(output_values.len());
            for ((output, a), b) in output_values
                .iter()
                .zip(left_values.iter())
                .zip(right_values.iter())
            {
                if *a == *b {
                    return Err(JetComputeError::Unsupported(
                        "maximum/minimum has no JVP at a tie".to_string(),
                    ));
                }
                if (maximum && *a == *output) || (!maximum && *a == *output) {
                    left_mask.push(1.0);
                    right_mask.push(0.0);
                } else {
                    left_mask.push(0.0);
                    right_mask.push(1.0);
                }
            }
            let left_mask = jet_compute_tensor_from_values_like(output, &left_mask)?;
            let right_mask = jet_compute_tensor_from_values_like(output, &right_mask)?;
            let left = jet_compute_binary("mul", &left_mask, &left_tangent)?;
            let right = jet_compute_binary("mul", &right_mask, &right_tangent)?;
            jet_compute_binary("add", &left, &right)
        }
        JetComputeTapeRule::Matmul => {
            let left = jet_compute_matmul(&tangents[0], &values[1])?;
            let right = jet_compute_matmul(&values[0], &tangents[1])?;
            jet_compute_binary("add", &left, &right)
        }
        JetComputeTapeRule::MatmulF32Tile => jet_compute_jvp_matmul_f32_tile(
            &values[0],
            &values[1],
            &tangents[0],
            &tangents[1],
        ),
        JetComputeTapeRule::MseLoss => jet_compute_mse_jvp(
            &values[0],
            &values[1],
            &tangents[0],
            &tangents[1],
        ),
        JetComputeTapeRule::SgdStep { learning_rate } => jet_compute_sgd_step(
            &tangents[0],
            &tangents[1],
            *learning_rate,
        ),
        JetComputeTapeRule::Unary(op) => jet_compute_unary_vjp(
            op,
            &values[0],
            output,
            &tangents[0],
        ),
        JetComputeTapeRule::Reshape { .. } => {
            jet_compute_reshape(&tangents[0], &output.shape)
        }
        JetComputeTapeRule::Broadcast { .. } => {
            jet_compute_broadcast_to(&tangents[0], &output.shape)
        }
        JetComputeTapeRule::ReduceToShape { .. } => {
            jet_compute_reduce_to_shape(&tangents[0], &output.shape)
        }
        JetComputeTapeRule::Transpose => jet_compute_transpose(&tangents[0]),
        JetComputeTapeRule::SumAxis { axis, .. } => jet_compute_sum_axis(&tangents[0], *axis as i64),
    }
}

fn jet_compute_jvp(
    state: &JetComputeVjpState,
    input_tangents: Vec<JetTensor>,
) -> Result<JetTensor, JetComputeError> {
    let (nodes, inputs) = {
        let tape = state
            .tape
            .lock()
            .map_err(|_| JetComputeError::Unsupported("autodiff tape is poisoned".to_string()))?;
        (tape.nodes.clone(), tape.inputs.clone())
    };
    if input_tangents.len() != inputs.len() {
        return Err(JetComputeError::RankMismatch(
            "JVP tangent count must equal the function input count".to_string(),
        ));
    }
    for (input, tangent) in inputs.iter().zip(input_tangents.iter()) {
        if input.shape != tangent.shape {
            return Err(JetComputeError::RankMismatch(
                "JVP tangent shapes must match their primal inputs".to_string(),
            ));
        }
        jet_compute_require_same_contract(input, tangent, "JVP tangent")?;
    }
    let mut tangents: Vec<Option<JetTensor>> = vec![None; nodes.len()];
    for (index, tangent) in input_tangents.into_iter().enumerate() {
        if let Some(slot) = tangents.get_mut(index) {
            *slot = Some(jet_compute_remove_trace_level(&tangent, &state.tape));
        }
    }
    for (index, node) in nodes.iter().enumerate().skip(inputs.len()) {
        let Some(rule) = &node.rule else {
            continue;
        };
        let mut node_tangents = Vec::with_capacity(node.parents.len());
        for (parent, value) in node.parents.iter().zip(node.values.iter()) {
            node_tangents.push(
                parent
                    .and_then(|parent| tangents.get(parent).and_then(Option::as_ref).cloned())
                    .unwrap_or(jet_compute_zero_like(value)?),
            );
        }
        tangents[index] = Some(jet_compute_jvp_rule(
            rule,
            &node.values,
            &node.output,
            &node_tangents,
        )?);
    }
    match state.output_node {
        Some(node) => tangents
            .get(node)
            .and_then(Option::clone)
            .ok_or_else(|| JetComputeError::Unsupported("JVP output tangent is unavailable".to_string())),
        None => jet_compute_zero_like(&state.value),
    }
}

fn jet_compute_jvp_or_panic(
    state: &JetComputeVjpState,
    input_tangents: Vec<JetTensor>,
    context: &str,
) -> JetTensor {
    match jet_compute_jvp(state, input_tangents) {
        Ok(value) => value,
        Err(error) => jet_panic("Compute.rs", line!(), &format!("{context}: {}", error.jet_show())),
    }
}

fn jet_compute_vjp_matmul(
    a: &JetTensor,
    b: &JetTensor,
    cot: &JetTensor,
) -> Result<(JetTensor, JetTensor), JetComputeError> {
    jet_compute_validate_tensor(a)?;
    jet_compute_validate_tensor(b)?;
    jet_compute_validate_tensor(cot)?;
    if a.shape.len() != 2
        || b.shape.len() != 2
        || cot.shape.len() != 2
        || a.shape[1] != b.shape[0]
        || cot.shape[0] != a.shape[0]
        || cot.shape[1] != b.shape[1]
    {
        return Err(JetComputeError::RankMismatch(
            "matmul cotangent shape must equal the matmul output".to_string(),
        ));
    }
    jet_compute_require_same_contract(a, b, "matmul cotangent")?;
    jet_compute_require_same_contract(a, cot, "matmul cotangent")?;
    let b_t = jet_compute_transpose(b)?;
    let a_t = jet_compute_transpose(a)?;
    Ok((
        jet_compute_inherit_placement(jet_compute_matmul(cot, &b_t)?, a),
        jet_compute_inherit_placement(jet_compute_matmul(&a_t, cot)?, b),
    ))
}

fn jet_compute_f32_projection(tensor: &JetTensor) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    let values = jet_compute_tensor_values(tensor)
        .into_iter()
        .map(|value| jet_compute_f32_value(value, "f32 autodiff input"))
        .collect::<Result<Vec<_>, _>>()?;
    let mut projected = jet_compute_tensor_from_shape_like(tensor, tensor.shape.clone(), 0.0)?;
    projected.data = std::sync::Arc::new(values.into_iter().map(f64::from).collect());
    if projected.device != JetComputeDevice::Metal {
        projected.last_placement.profile = CPU_ORACLE_F32_PROFILE.to_string();
        projected.last_placement.abilities = CPU_ORACLE_F32_CAPABILITIES
            .iter()
            .map(|ability| (*ability).to_string())
            .collect();
        projected.last_placement.reason = "autodiff f32 projection".to_string();
    }
    jet_compute_validate_tensor(&projected)?;
    Ok(projected)
}

fn jet_compute_vjp_matmul_f32_tile(
    a: &JetTensor,
    b: &JetTensor,
    cot: &JetTensor,
) -> Result<(JetTensor, JetTensor), JetComputeError> {
    jet_compute_validate_tensor(a)?;
    jet_compute_validate_tensor(b)?;
    jet_compute_validate_tensor(cot)?;
    if a.shape.len() != 2
        || b.shape.len() != 2
        || cot.shape.len() != 2
        || a.shape[1] != b.shape[0]
        || cot.shape[0] != a.shape[0]
        || cot.shape[1] != b.shape[1]
    {
        return Err(JetComputeError::RankMismatch(
            "matmul_f32_tile cotangent shape must equal the matmul output".to_string(),
        ));
    }
    if a.device != b.device || a.device != cot.device {
        return Err(JetComputeError::Device(
            "matmul_f32_tile cotangent devices must match the inputs".to_string(),
        ));
    }
    let a32 = jet_compute_f32_projection(a)?;
    let b32 = jet_compute_f32_projection(b)?;
    let cot32 = jet_compute_f32_projection(cot)?;
    let b_transposed = jet_compute_transpose(&b32)?;
    let a_transposed = jet_compute_transpose(&a32)?;
    let a_gradient = jet_compute_matmul_f32_tile(&cot32, &b_transposed)?;
    let b_gradient = jet_compute_matmul_f32_tile(&a_transposed, &cot32)?;
    Ok((
        jet_compute_tensor_from_values_like(a, &jet_compute_tensor_values(&a_gradient))?,
        jet_compute_tensor_from_values_like(b, &jet_compute_tensor_values(&b_gradient))?,
    ))
}

fn jet_compute_jvp_matmul_f32_tile(
    a: &JetTensor,
    b: &JetTensor,
    a_tangent: &JetTensor,
    b_tangent: &JetTensor,
) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(a)?;
    jet_compute_validate_tensor(b)?;
    jet_compute_validate_tensor(a_tangent)?;
    jet_compute_validate_tensor(b_tangent)?;
    if a.shape != a_tangent.shape || b.shape != b_tangent.shape {
        return Err(JetComputeError::RankMismatch(
            "matmul_f32_tile tangent shapes must match the inputs".to_string(),
        ));
    }
    if a.device != b.device
        || a.device != a_tangent.device
        || b.device != b_tangent.device
    {
        return Err(JetComputeError::Device(
            "matmul_f32_tile tangent devices must match the inputs".to_string(),
        ));
    }
    let a32 = jet_compute_f32_projection(a)?;
    let b32 = jet_compute_f32_projection(b)?;
    let a_tangent32 = jet_compute_f32_projection(a_tangent)?;
    let b_tangent32 = jet_compute_f32_projection(b_tangent)?;
    let left = jet_compute_matmul_f32_tile(&a_tangent32, &b32)?;
    let right = jet_compute_matmul_f32_tile(&a32, &b_tangent32)?;
    jet_compute_binary("add", &left, &right)
}

fn jet_compute_f32_value(value: f64, context: &str) -> Result<f32, JetComputeError> {
    let narrowed = value as f32;
    if !narrowed.is_finite() {
        return Err(JetComputeError::Arithmetic(format!(
            "{context} is outside the finite F32 range"
        )));
    }
    Ok(narrowed)
}

fn jet_compute_validate_profile_value(
    profile: &str,
    value: f64,
    context: &str,
) -> Result<(), JetComputeError> {
    if profile == CPU_ORACLE_F32_PROFILE {
        let narrowed = jet_compute_f32_value(value, context)?;
        if f64::from(narrowed) != value {
            return Err(JetComputeError::Arithmetic(format!(
                "{context} is not canonical for the F32 precision profile"
            )));
        }
    }
    Ok(())
}

fn jet_compute_validate_serialized_profile_values(
    profile: &str,
    values: &[f64],
) -> Result<(), JetComputeError> {
    if profile == CPU_ORACLE_F32_PROFILE {
        for value in values {
            let narrowed = jet_compute_f32_value(*value, "serialized Tensor value")?;
            if f64::from(narrowed) != *value {
                return Err(JetComputeError::Serialization(
                    "serialized Tensor value is not canonical for its F32 profile".to_string(),
                ));
            }
        }
    }
    Ok(())
}

fn jet_compute_wire_checksum(body: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in body.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3_u64);
    }
    format!("{hash:016x}")
}

// ── #1142: ML training/inference + model serialization over the Tensor oracle ─

fn jet_compute_validate_mse_inputs(
    pred: &JetTensor,
    target: &JetTensor,
) -> Result<(Vec<f64>, Vec<f64>), JetComputeError> {
    jet_compute_validate_tensor(pred)?;
    jet_compute_validate_tensor(target)?;
    if pred.shape != target.shape {
        return Err(JetComputeError::RankMismatch(
            "mse_loss prediction and target shapes must match".to_string(),
        ));
    }
    if pred.device != target.device {
        return Err(JetComputeError::Device(
            "mse_loss prediction and target devices must match".to_string(),
        ));
    }
    if pred.last_placement.profile != target.last_placement.profile {
        return Err(JetComputeError::Device(
            "mse_loss prediction and target precision profiles must match".to_string(),
        ));
    }
    let pred_values = jet_compute_tensor_values(pred);
    let target_values = jet_compute_tensor_values(target);
    if pred_values.is_empty() {
        return Err(JetComputeError::InvalidShape(
            "mse_loss requires a non-empty tensor".to_string(),
        ));
    }
    Ok((pred_values, target_values))
}

fn jet_compute_scalar_from_like(
    template: &JetTensor,
    value: f64,
) -> Result<JetTensor, JetComputeError> {
    if !value.is_finite() {
        return Err(JetComputeError::Arithmetic(
            "compute scalar produced a non-finite value".to_string(),
        ));
    }
    let output = jet_compute_inherit_placement(
        jet_compute_tensor_from_shape(vec![1], value, JetComputeDevice::Cpu)?,
        template,
    );
    jet_compute_validate_tensor(&output)?;
    Ok(output)
}

fn jet_compute_mse_value(
    pred: &JetTensor,
    pred_values: &[f64],
    target_values: &[f64],
) -> Result<f64, JetComputeError> {
    let loss = if pred.last_placement.profile == CPU_ORACLE_F32_PROFILE {
        let mut sum = 0.0_f32;
        for (pred_value, target_value) in pred_values.iter().zip(target_values.iter()) {
            let pred_value = jet_compute_f32_value(*pred_value, "mse_loss prediction")?;
            let target_value = jet_compute_f32_value(*target_value, "mse_loss target")?;
            let difference = pred_value - target_value;
            let next = sum + difference * difference;
            if !next.is_finite() {
                return Err(JetComputeError::Arithmetic(
                    "mse_loss accumulated a non-finite value".to_string(),
                ));
            }
            sum = next;
        }
        f64::from(sum / pred_values.len() as f32)
    } else {
        let mut sum = 0.0_f64;
        for (pred_value, target_value) in pred_values.iter().zip(target_values.iter()) {
            let difference = *pred_value - *target_value;
            let next = sum + difference * difference;
            if !next.is_finite() {
                return Err(JetComputeError::Arithmetic(
                    "mse_loss accumulated a non-finite value".to_string(),
                ));
            }
            sum = next;
        }
        sum / pred_values.len() as f64
    };
    if !loss.is_finite() {
        return Err(JetComputeError::Arithmetic(
            "mse_loss produced a non-finite value".to_string(),
        ));
    }
    Ok(loss)
}

/// MSE is a scalar Tensor operation, not a host float reduction. Recording it
/// here keeps eager loss, transformed loss, and all execution tiers on one
/// VJP/JVP rule.
fn jet_compute_mse_loss(
    pred: &JetTensor,
    target: &JetTensor,
) -> Result<JetTensor, JetComputeError> {
    let (pred_values, target_values) = jet_compute_validate_mse_inputs(pred, target)?;
    if pred.device == JetComputeDevice::Metal {
        let pred_values = jet_compute_metal_values(pred, "MSE input")?;
        let target_values = jet_compute_metal_values(target, "MSE input")?;
        let loss = jet_compute_metal::mse(&pred_values, &target_values)?
            .into_iter()
            .next()
            .ok_or_else(|| JetComputeError::Device("Metal MSE returned no value".to_string()))?;
        return jet_compute_record(
            jet_compute_scalar_from_like(pred, f64::from(loss))?,
            &[pred, target],
            vec![pred.clone(), target.clone()],
            JetComputeTapeRule::MseLoss,
        );
    }
    let loss = jet_compute_mse_value(pred, &pred_values, &target_values)?;
    jet_compute_record(
        jet_compute_scalar_from_like(pred, loss)?,
        &[pred, target],
        vec![pred.clone(), target.clone()],
        JetComputeTapeRule::MseLoss,
    )
}

fn jet_compute_mse_vjp(
    pred: &JetTensor,
    target: &JetTensor,
    cot: &JetTensor,
    positive: bool,
) -> Result<JetTensor, JetComputeError> {
    let (pred_values, target_values) = jet_compute_validate_mse_inputs(pred, target)?;
    jet_compute_validate_tensor(cot)?;
    if cot.shape != vec![1] {
        return Err(JetComputeError::RankMismatch(
            "mse_loss cotangent must be a scalar Tensor".to_string(),
        ));
    }
    jet_compute_require_same_contract(pred, cot, "mse_loss cotangent")?;
    let cot_value = jet_compute_tensor_values(cot)
        .first()
        .copied()
        .ok_or_else(|| JetComputeError::InvalidShape("mse_loss cotangent is empty".to_string()))?;
    if pred.device == JetComputeDevice::Metal {
        let pred_values = jet_compute_metal_values(pred, "MSE gradient input")?;
        let target_values = jet_compute_metal_values(target, "MSE gradient input")?;
        let cot_value = jet_compute_f32_value(cot_value, "mse_loss cotangent")?;
        let data = jet_compute_metal::mse_grad(
            &pred_values,
            &target_values,
            &[cot_value],
            positive,
        )?;
        return jet_compute_tensor_from_values_like(
            pred,
            &data.into_iter().map(f64::from).collect::<Vec<_>>(),
        );
    }
    let data = if pred.last_placement.profile == CPU_ORACLE_F32_PROFILE {
        let cot_value = jet_compute_f32_value(cot_value, "mse_loss cotangent")?;
        let factor = 2.0_f32 / pred_values.len() as f32 * cot_value;
        if !factor.is_finite() {
            return Err(JetComputeError::Arithmetic(
                "mse_loss gradient factor is non-finite".to_string(),
            ));
        }
        pred_values
            .iter()
            .zip(target_values.iter())
            .map(|(pred_value, target_value)| {
                let pred_value = jet_compute_f32_value(*pred_value, "mse_loss prediction")?;
                let target_value = jet_compute_f32_value(*target_value, "mse_loss target")?;
                let difference = if positive {
                    pred_value - target_value
                } else {
                    target_value - pred_value
                };
                let value = difference * factor;
                value.is_finite().then_some(f64::from(value)).ok_or_else(|| {
                    JetComputeError::Arithmetic(
                        "mse_loss gradient produced a non-finite value".to_string(),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        let factor = 2.0 / pred_values.len() as f64 * cot_value;
        if !factor.is_finite() {
            return Err(JetComputeError::Arithmetic(
                "mse_loss gradient factor is non-finite".to_string(),
            ));
        }
        pred_values
            .iter()
            .zip(target_values.iter())
            .map(|(pred_value, target_value)| {
                let difference = if positive {
                    *pred_value - *target_value
                } else {
                    *target_value - *pred_value
                };
                let value = difference * factor;
                value.is_finite().then_some(value).ok_or_else(|| {
                    JetComputeError::Arithmetic(
                        "mse_loss gradient produced a non-finite value".to_string(),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    jet_compute_tensor_from_values_like(pred, &data)
}

fn jet_compute_mse_jvp(
    pred: &JetTensor,
    target: &JetTensor,
    pred_tangent: &JetTensor,
    target_tangent: &JetTensor,
) -> Result<JetTensor, JetComputeError> {
    let (pred_values, target_values) = jet_compute_validate_mse_inputs(pred, target)?;
    jet_compute_validate_tensor(pred_tangent)?;
    jet_compute_validate_tensor(target_tangent)?;
    if pred_tangent.shape != pred.shape || target_tangent.shape != target.shape {
        return Err(JetComputeError::RankMismatch(
            "mse_loss tangent shapes must match their primal tensors".to_string(),
        ));
    }
    if pred_tangent.last_placement.profile != pred.last_placement.profile
        || target_tangent.last_placement.profile != target.last_placement.profile
    {
        return Err(JetComputeError::Device(
            "mse_loss tangent precision profiles must match their primal tensors".to_string(),
        ));
    }
    jet_compute_require_same_contract(pred, pred_tangent, "mse_loss tangent")?;
    jet_compute_require_same_contract(target, target_tangent, "mse_loss tangent")?;
    let pred_tangent_values = jet_compute_tensor_values(pred_tangent);
    let target_tangent_values = jet_compute_tensor_values(target_tangent);
    if pred.device == JetComputeDevice::Metal {
        let pred_values = jet_compute_metal_values(pred, "MSE JVP input")?;
        let target_values = jet_compute_metal_values(target, "MSE JVP input")?;
        let pred_tangent_values = jet_compute_metal_values(pred_tangent, "MSE JVP tangent")?;
        let target_tangent_values = jet_compute_metal_values(target_tangent, "MSE JVP tangent")?;
        let value = jet_compute_metal::mse_jvp(
            &pred_values,
            &target_values,
            &pred_tangent_values,
            &target_tangent_values,
        )?
        .into_iter()
        .next()
        .ok_or_else(|| JetComputeError::Device("Metal MSE JVP returned no value".to_string()))?;
        return jet_compute_scalar_from_like(pred, f64::from(value));
    }
    let value = if pred.last_placement.profile == CPU_ORACLE_F32_PROFILE {
        let mut sum = 0.0_f32;
        for (((pred_value, target_value), pred_tangent), target_tangent) in pred_values
            .iter()
            .zip(target_values.iter())
            .zip(pred_tangent_values.iter())
            .zip(target_tangent_values.iter())
        {
            let difference = jet_compute_f32_value(*pred_value, "mse_loss prediction")?
                - jet_compute_f32_value(*target_value, "mse_loss target")?;
            let direction = jet_compute_f32_value(*pred_tangent, "mse_loss prediction tangent")?
                - jet_compute_f32_value(*target_tangent, "mse_loss target tangent")?;
            let next = sum + 2.0_f32 * difference * direction;
            if !next.is_finite() {
                return Err(JetComputeError::Arithmetic(
                    "mse_loss JVP accumulated a non-finite value".to_string(),
                ));
            }
            sum = next;
        }
        f64::from(sum / pred_values.len() as f32)
    } else {
        let mut sum = 0.0_f64;
        for (((pred_value, target_value), pred_tangent), target_tangent) in pred_values
            .iter()
            .zip(target_values.iter())
            .zip(pred_tangent_values.iter())
            .zip(target_tangent_values.iter())
        {
            let next = sum
                + 2.0
                    * (*pred_value - *target_value)
                    * (*pred_tangent - *target_tangent);
            if !next.is_finite() {
                return Err(JetComputeError::Arithmetic(
                    "mse_loss JVP accumulated a non-finite value".to_string(),
                ));
            }
            sum = next;
        }
        sum / pred_values.len() as f64
    };
    jet_compute_scalar_from_like(pred, value)
}

fn jet_compute_sgd_step(
    param: &JetTensor,
    grad: &JetTensor,
    lr: f64,
) -> Result<JetTensor, JetComputeError> {
    jet_compute_validate_tensor(param)?;
    jet_compute_validate_tensor(grad)?;
    if param.shape != grad.shape {
        return Err(JetComputeError::RankMismatch(
            "sgd parameter and gradient shapes must match".to_string(),
        ));
    }
    if param.device != grad.device {
        return Err(JetComputeError::Device(
            "sgd parameter and gradient devices must match".to_string(),
        ));
    }
    if param.last_placement.profile != grad.last_placement.profile {
        return Err(JetComputeError::Device(
            "sgd parameter and gradient precision profiles must match".to_string(),
        ));
    }
    if !lr.is_finite() || lr < 0.0 {
        return Err(JetComputeError::Arithmetic(
            "sgd learning rate must be finite and non-negative".to_string(),
        ));
    }
    if param.device == JetComputeDevice::Metal {
        let parameter = jet_compute_metal_values(param, "SGD input")?;
        let gradient = jet_compute_metal_values(grad, "SGD input")?;
        let learning_rate = jet_compute_f32_value(lr, "sgd learning rate")?;
        let data = jet_compute_metal::sgd(&parameter, &gradient, learning_rate)?;
        return jet_compute_record(
            jet_compute_metal_result_like(param, param.shape.clone(), data)?,
            &[param, grad],
            vec![param.clone(), grad.clone()],
            JetComputeTapeRule::SgdStep {
                learning_rate: lr,
            },
        );
    }

    let param_values = jet_compute_tensor_values(param);
    let grad_values = jet_compute_tensor_values(grad);
    let data = if param.last_placement.profile == CPU_ORACLE_F32_PROFILE {
        let learning_rate = jet_compute_f32_value(lr, "sgd learning rate")?;
        param_values
            .iter()
            .zip(grad_values.iter())
            .map(|(param_value, grad_value)| {
                let param_value = jet_compute_f32_value(*param_value, "sgd parameter")?;
                let grad_value = jet_compute_f32_value(*grad_value, "sgd gradient")?;
                let next = param_value - learning_rate * grad_value;
                next.is_finite().then_some(f64::from(next)).ok_or_else(|| {
                    JetComputeError::Arithmetic(
                        "sgd_step produced a non-finite value".to_string(),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        param_values
            .iter()
            .zip(grad_values.iter())
            .map(|(param_value, grad_value)| {
                let next = *param_value - lr * *grad_value;
                next.is_finite().then_some(next).ok_or_else(|| {
                    JetComputeError::Arithmetic(
                        "sgd_step produced a non-finite value".to_string(),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    let next = JetTensor {
        shape: param.shape.clone(),
        strides: jet_compute_row_major_strides(&param.shape)?,
        data: std::sync::Arc::new(data),
        device: param.device,
        last_placement: param.last_placement.clone(),
        last_transfer: None,
        trace: None,
    };
    jet_compute_validate_tensor(&next)?;
    jet_compute_record(
        next,
        &[param, grad],
        vec![param.clone(), grad.clone()],
        JetComputeTapeRule::SgdStep {
            learning_rate: lr,
        },
    )
}

fn jet_compute_sgd_vjp(
    param: &JetTensor,
    grad: &JetTensor,
    cot: &JetTensor,
    lr: f64,
) -> Result<(JetTensor, JetTensor), JetComputeError> {
    jet_compute_validate_tensor(param)?;
    jet_compute_validate_tensor(grad)?;
    jet_compute_validate_tensor(cot)?;
    if param.shape != grad.shape || param.shape != cot.shape {
        return Err(JetComputeError::RankMismatch(
            "sgd cotangent shape must equal the parameter and gradient shapes".to_string(),
        ));
    }
    if param.device != grad.device
        || param.device != cot.device
        || param.last_placement.profile != grad.last_placement.profile
        || param.last_placement.profile != cot.last_placement.profile
    {
        return Err(JetComputeError::Device(
            "sgd parameter, gradient, and cotangent devices and profiles must match".to_string(),
        ));
    }
    if !lr.is_finite() || lr < 0.0 {
        return Err(JetComputeError::Arithmetic(
            "sgd learning rate must be finite and non-negative".to_string(),
        ));
    }
    let cot_values = jet_compute_tensor_values(cot);
    if param.device == JetComputeDevice::Metal {
        let cot_values = jet_compute_metal_values(cot, "SGD cotangent")?;
        let learning_rate = jet_compute_f32_value(lr, "sgd learning rate")?;
        let gradients = jet_compute_metal::scale(&cot_values, -learning_rate)?;
        return Ok((
            jet_compute_tensor_from_values_like(
                param,
                &cot_values.into_iter().map(f64::from).collect::<Vec<_>>(),
            )?,
            jet_compute_tensor_from_values_like(
                grad,
                &gradients.into_iter().map(f64::from).collect::<Vec<_>>(),
            )?,
        ));
    }
    let (parameter_values, gradient_values) = if param.last_placement.profile == CPU_ORACLE_F32_PROFILE {
        let learning_rate = jet_compute_f32_value(lr, "sgd learning rate")?;
        let mut parameters = Vec::with_capacity(cot_values.len());
        let mut gradients = Vec::with_capacity(cot_values.len());
        for cot_value in cot_values {
            let cot_value = jet_compute_f32_value(cot_value, "sgd cotangent")?;
            let gradient = -learning_rate * cot_value;
            if !gradient.is_finite() {
                return Err(JetComputeError::Arithmetic(
                    "sgd gradient produced a non-finite value".to_string(),
                ));
            }
            parameters.push(f64::from(cot_value));
            gradients.push(f64::from(gradient));
        }
        (parameters, gradients)
    } else {
        let gradients = cot_values
            .iter()
            .map(|cot_value| {
                let value = -lr * *cot_value;
                value.is_finite().then_some(value).ok_or_else(|| {
                    JetComputeError::Arithmetic(
                        "sgd gradient produced a non-finite value".to_string(),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        (cot_values, gradients)
    };
    Ok((
        jet_compute_tensor_from_values_like(param, &parameter_values)?,
        jet_compute_tensor_from_values_like(grad, &gradient_values)?,
    ))
}

fn jet_compute_serialize(tensor: &JetTensor) -> Result<String, JetComputeError> {
    jet_compute_validate_tensor(tensor)?;
    if tensor.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "Tensor serialization does not accept traced tensors".to_string(),
        ));
    }
    if tensor.device == JetComputeDevice::Metal {
        return Err(JetComputeError::Unsupported(
            "Metal Tensor serialization requires an explicit transfer to CPU".to_string(),
        ));
    }
    let values = jet_compute_tensor_values(tensor);
    let shape = tensor
        .shape
        .iter()
        .map(|d| d.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let data = values
        .iter()
        // Debug formatting is Rust's shortest round-tripping f64 spelling.
        // Keep it stable across the AOT/JIT/interpreter Prelude boundary.
        .map(|v| format!("{v:?}"))
        .collect::<Vec<_>>()
        .join(",");
    let profile = tensor.last_placement.profile.as_str();
    jet_compute_validate_serialized_profile_values(profile, &values)?;
    let body = format!("shape={shape};data={data};profile={profile}");
    let checksum = jet_compute_wire_checksum(&body);
    Ok(format!("{body};checksum={checksum}"))
}

fn jet_compute_deserialize(payload: &String) -> Result<JetTensor, JetComputeError> {
    let mut fields = payload.split(';');
    let Some(shape_part) = fields.next() else {
        return Err(JetComputeError::InvalidShape(
            "deserialize expects shape=…;data=…;profile=…;checksum=…".to_string(),
        ));
    };
    let Some(data_part) = fields.next() else {
        return Err(JetComputeError::InvalidShape(
            "deserialize expects shape=…;data=…;profile=…;checksum=…".to_string(),
        ));
    };
    let Some(profile_part) = fields.next() else {
        return Err(JetComputeError::InvalidShape(
            "deserialize expects shape=…;data=…;profile=…;checksum=…".to_string(),
        ));
    };
    let Some(checksum_part) = fields.next() else {
        return Err(JetComputeError::InvalidShape(
            "deserialize expects shape=…;data=…;profile=…;checksum=…".to_string(),
        ));
    };
    if fields.next().is_some()
        || !data_part.starts_with("data=")
        || !profile_part.starts_with("profile=")
        || !checksum_part.starts_with("checksum=")
    {
        return Err(JetComputeError::Serialization(
            "deserialize contains duplicate or unknown fields".to_string(),
        ));
    }
    let shape_str = shape_part
        .strip_prefix("shape=")
        .ok_or_else(|| JetComputeError::Serialization("missing shape=".to_string()))?;
    if shape_str.is_empty() {
        return Err(JetComputeError::Serialization(
            "serialized Tensor shape cannot be empty".to_string(),
        ));
    }
    let shape: Vec<i64> = shape_str
        .split(',')
        .map(|p| {
            if p.is_empty() {
                return Err(JetComputeError::Serialization(
                    "serialized Tensor shape contains an empty axis".to_string(),
                ));
            }
            let axis = p.parse::<i64>().map_err(|_| {
                JetComputeError::Serialization(format!("bad shape axis `{p}`"))
            })?;
            if axis.to_string() != p {
                return Err(JetComputeError::Serialization(format!(
                    "non-canonical shape axis `{p}`"
                )));
            }
            Ok(axis)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let data_str = data_part.strip_prefix("data=").unwrap_or("");
    let data: Vec<f64> = if data_str.is_empty() {
        Vec::new()
    } else {
        data_str
            .split(',')
            .map(|p| {
                if p.is_empty() {
                    return Err(JetComputeError::Serialization(
                        "serialized Tensor data contains an empty value".to_string(),
                    ));
                }
                let value = p.parse::<f64>().map_err(|_| {
                    JetComputeError::Serialization(format!("bad data value `{p}`"))
                })?;
                if !value.is_finite() {
                    return Err(JetComputeError::Serialization(
                        "serialized Tensor contains a non-finite value".to_string(),
                    ));
                }
                if format!("{value:?}") != p {
                    return Err(JetComputeError::Serialization(format!(
                        "non-canonical data value `{p}`"
                    )));
                }
                Ok(value)
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    let profile = profile_part.strip_prefix("profile=").unwrap_or("");
    let Some(abilities) = jet_compute_registered_abilities(profile) else {
        return Err(JetComputeError::Serialization(format!(
            "unsupported Tensor precision profile `{profile}`"
        )));
    };
    jet_compute_validate_serialized_profile_values(profile, &data)?;
    let checksum = checksum_part.strip_prefix("checksum=").unwrap_or("");
    if checksum.len() != 16
        || !checksum
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(JetComputeError::Serialization(
            "serialized Tensor checksum is not canonical".to_string(),
        ));
    }
    let body = format!("{shape_part};{data_part};{profile_part}");
    if jet_compute_wire_checksum(&body) != checksum {
        return Err(JetComputeError::Serialization(
            "serialized Tensor checksum does not match its contents".to_string(),
        ));
    }
    let expected = jet_compute_storage_len(&shape)?;
    if expected != data.len() {
        return Err(JetComputeError::Serialization(format!(
            "deserialize storage length mismatch: shape wants {expected}, got {}",
            data.len()
        )));
    }
    let mut tensor = jet_compute_tensor_from_shape(shape, 0.0, JetComputeDevice::Cpu)?;
    tensor.data = std::sync::Arc::new(data);
    tensor.last_placement.profile = profile.to_string();
    tensor.last_placement.abilities = abilities
        .iter()
        .map(|ability| (*ability).to_string())
        .collect();
    tensor.last_placement.reason = "deserialized canonical Tensor".to_string();
    jet_compute_validate_tensor(&tensor)?;
    Ok(tensor)
}

// ── #1137 sparse CSR + #1143 CPU SIMD tile + #1147 profile ──────────────────

#[derive(Clone, Debug, PartialEq)]
pub struct JetSparseCsr {
    rows: i64,
    cols: i64,
    row_ptr: Vec<i64>,
    col_idx: Vec<i64>,
    values: Vec<f64>,
}

impl JetShow for JetSparseCsr {
    fn jet_show(&self) -> String {
        format!(
            "SparseCsr({}x{}, nnz={})",
            self.rows,
            self.cols,
            self.values.len()
        )
    }
}

fn jet_compute_to_sparse(tensor: &JetTensor) -> Result<JetSparseCsr, JetComputeError> {
    if tensor.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "to_sparse has no registered autodiff rule".to_string(),
        ));
    }
    if tensor.shape.len() != 2 {
        return Err(JetComputeError::RankMismatch(
            "to_sparse requires a rank-2 tensor".to_string(),
        ));
    }
    jet_compute_validate_tensor(tensor)?;
    if tensor.device == JetComputeDevice::Metal {
        return Err(JetComputeError::Unsupported(
            "Metal backend does not support sparse conversion; transfer to CPU explicitly"
                .to_string(),
        ));
    }
    let rows = tensor.shape[0];
    let cols = tensor.shape[1];
    let mut row_ptr = vec![0i64];
    let mut col_idx = Vec::new();
    let mut values = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            let v = jet_compute_get_raw(tensor, &vec![r, c])?;
            if v != 0.0 {
                col_idx.push(c);
                values.push(v);
            }
        }
        row_ptr.push(i64::try_from(values.len()).map_err(|_| {
            JetComputeError::InvalidShape("sparse nnz is too large".to_string())
        })?);
    }
    Ok(JetSparseCsr {
        rows,
        cols,
        row_ptr,
        col_idx,
        values,
    })
}

fn jet_compute_sparse_nnz(sparse: &JetSparseCsr) -> i64 {
    i64::try_from(sparse.values.len()).unwrap_or(i64::MAX)
}

fn jet_compute_sparse_mv(
    sparse: &JetSparseCsr,
    vector: &JetTensor,
) -> Result<JetTensor, JetComputeError> {
    if vector.trace.is_some() {
        return Err(JetComputeError::Unsupported(
            "sparse_mv has no registered autodiff rule".to_string(),
        ));
    }
    jet_compute_validate_sparse(sparse)?;
    jet_compute_validate_tensor(vector)?;
    if vector.device == JetComputeDevice::Metal {
        return Err(JetComputeError::Unsupported(
            "Metal backend does not support sparse_mv; transfer the vector to CPU explicitly"
                .to_string(),
        ));
    }
    if vector.shape.len() != 1 || vector.shape[0] != sparse.cols {
        return Err(JetComputeError::RankMismatch(format!(
            "sparse_mv expects a length-{} vector",
            sparse.cols
        )));
    }
    let mut out = jet_compute_zeros(&vec![sparse.rows])?;
    for r in 0..sparse.rows {
        let row = usize::try_from(r).map_err(|_| {
            JetComputeError::InvalidShape("sparse row index is too large".to_string())
        })?;
        let start = usize::try_from(sparse.row_ptr[row]).map_err(|_| {
            JetComputeError::InvalidShape("sparse row pointer is invalid".to_string())
        })?;
        let end = usize::try_from(sparse.row_ptr[row + 1]).map_err(|_| {
            JetComputeError::InvalidShape("sparse row pointer is invalid".to_string())
        })?;
        let mut acc = 0.0;
        for k in start..end {
            let c = sparse.col_idx[k];
            acc += sparse.values[k] * jet_compute_get_raw(vector, &[c])?;
            if !acc.is_finite() {
                return Err(JetComputeError::Arithmetic(
                    "sparse matrix-vector multiplication produced a non-finite value"
                        .to_string(),
                ));
            }
        }
        jet_compute_set(&mut out, &vec![r], acc)?;
    }
    Ok(out)
}

fn jet_compute_validate_sparse(sparse: &JetSparseCsr) -> Result<(), JetComputeError> {
    if sparse.rows < 0 || sparse.cols < 0 {
        return Err(JetComputeError::InvalidShape(
            "sparse dimensions must be non-negative".to_string(),
        ));
    }
    let rows = usize::try_from(sparse.rows).map_err(|_| {
        JetComputeError::InvalidShape("sparse row count is too large".to_string())
    })?;
    if sparse.row_ptr.len() != rows.saturating_add(1)
        || sparse.col_idx.len() != sparse.values.len()
    {
        return Err(JetComputeError::InvalidShape(
            "sparse CSR arrays have inconsistent lengths".to_string(),
        ));
    }
    let nnz = i64::try_from(sparse.values.len()).map_err(|_| {
        JetComputeError::InvalidShape("sparse nnz is too large".to_string())
    })?;
    if sparse.row_ptr.first().copied() != Some(0)
        || sparse.row_ptr.last().copied() != Some(nnz)
        || sparse
            .row_ptr
            .windows(2)
            .any(|pair| pair[0] < 0 || pair[1] < pair[0] || pair[1] > nnz)
        || sparse
            .col_idx
            .iter()
            .any(|col| *col < 0 || *col >= sparse.cols)
        || sparse.values.iter().any(|value| !value.is_finite())
    {
        return Err(JetComputeError::InvalidShape(
            "sparse CSR invariants are invalid".to_string(),
        ));
    }
    Ok(())
}

fn jet_compute_sparse_show(sparse: &JetSparseCsr) -> String {
    sparse.jet_show()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JetComputeSimdBackend {
    Avx2,
    Sse2,
    Scalar,
}

impl JetComputeSimdBackend {
    fn name(self) -> &'static str {
        match self {
            Self::Avx2 => "avx2",
            Self::Sse2 => "sse2",
            Self::Scalar => "scalar",
        }
    }

    fn width(self) -> usize {
        match self {
            Self::Avx2 => 8,
            Self::Sse2 => 4,
            Self::Scalar => 1,
        }
    }
}

fn jet_compute_simd_backend() -> JetComputeSimdBackend {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        return jet_compute_simd_backend_for_features(
            is_x86_feature_detected!("avx2"),
            is_x86_feature_detected!("sse2"),
        );
    }
    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
    {
        jet_compute_simd_backend_for_features(false, false)
    }
}

fn jet_compute_simd_backend_for_features(avx2: bool, sse2: bool) -> JetComputeSimdBackend {
    if avx2 {
        return JetComputeSimdBackend::Avx2;
    }
    if sse2 {
        return JetComputeSimdBackend::Sse2;
    }
    JetComputeSimdBackend::Scalar
}

#[inline(never)]
fn jet_compute_f32_dot_scalar(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b.iter())
        .fold(0.0_f32, |sum, (left, right)| sum + left * right)
}

fn jet_compute_simd_backend_available(backend: JetComputeSimdBackend) -> bool {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        match backend {
            JetComputeSimdBackend::Avx2 => is_x86_feature_detected!("avx2"),
            JetComputeSimdBackend::Sse2 => is_x86_feature_detected!("sse2"),
            JetComputeSimdBackend::Scalar => true,
        }
    }
    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
    {
        matches!(backend, JetComputeSimdBackend::Scalar)
    }
}

// JET_VETTED_UNSAFE_BEGIN: jet_compute_cpu_simd
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn jet_compute_f32_dot_avx2(a: &[f32], b: &[f32]) -> f32 {
    use std::arch::x86_64::{_mm256_loadu_ps, _mm256_mul_ps, _mm256_storeu_ps};

    let mut sum = 0.0_f32;
    let mut index = 0usize;
    let limit = a.len() / 8 * 8;
    while index < limit {
        let left = _mm256_loadu_ps(a.as_ptr().add(index));
        let right = _mm256_loadu_ps(b.as_ptr().add(index));
        let product = _mm256_mul_ps(left, right);
        let mut lanes = [0.0_f32; 8];
        _mm256_storeu_ps(lanes.as_mut_ptr(), product);
        for lane in lanes {
            sum += lane;
        }
        index += 8;
    }
    while index < a.len() {
        sum += a[index] * b[index];
        index += 1;
    }
    sum
}

#[cfg(target_arch = "x86")]
#[target_feature(enable = "avx2")]
unsafe fn jet_compute_f32_dot_avx2(a: &[f32], b: &[f32]) -> f32 {
    use std::arch::x86::{_mm256_loadu_ps, _mm256_mul_ps, _mm256_storeu_ps};

    let mut sum = 0.0_f32;
    let mut index = 0usize;
    let limit = a.len() / 8 * 8;
    while index < limit {
        let left = _mm256_loadu_ps(a.as_ptr().add(index));
        let right = _mm256_loadu_ps(b.as_ptr().add(index));
        let product = _mm256_mul_ps(left, right);
        let mut lanes = [0.0_f32; 8];
        _mm256_storeu_ps(lanes.as_mut_ptr(), product);
        for lane in lanes {
            sum += lane;
        }
        index += 8;
    }
    while index < a.len() {
        sum += a[index] * b[index];
        index += 1;
    }
    sum
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn jet_compute_f32_dot_sse2(a: &[f32], b: &[f32]) -> f32 {
    use std::arch::x86_64::{_mm_loadu_ps, _mm_mul_ps, _mm_storeu_ps};

    let mut sum = 0.0_f32;
    let mut index = 0usize;
    let limit = a.len() / 4 * 4;
    while index < limit {
        let left = _mm_loadu_ps(a.as_ptr().add(index));
        let right = _mm_loadu_ps(b.as_ptr().add(index));
        let product = _mm_mul_ps(left, right);
        let mut lanes = [0.0_f32; 4];
        _mm_storeu_ps(lanes.as_mut_ptr(), product);
        for lane in lanes {
            sum += lane;
        }
        index += 4;
    }
    while index < a.len() {
        sum += a[index] * b[index];
        index += 1;
    }
    sum
}

#[cfg(target_arch = "x86")]
#[target_feature(enable = "sse2")]
unsafe fn jet_compute_f32_dot_sse2(a: &[f32], b: &[f32]) -> f32 {
    use std::arch::x86::{_mm_loadu_ps, _mm_mul_ps, _mm_storeu_ps};

    let mut sum = 0.0_f32;
    let mut index = 0usize;
    let limit = a.len() / 4 * 4;
    while index < limit {
        let left = _mm_loadu_ps(a.as_ptr().add(index));
        let right = _mm_loadu_ps(b.as_ptr().add(index));
        let product = _mm_mul_ps(left, right);
        let mut lanes = [0.0_f32; 4];
        _mm_storeu_ps(lanes.as_mut_ptr(), product);
        for lane in lanes {
            sum += lane;
        }
        index += 4;
    }
    while index < a.len() {
        sum += a[index] * b[index];
        index += 1;
    }
    sum
}
fn jet_compute_f32_dot(
    backend: JetComputeSimdBackend,
    a: &[f32],
    b: &[f32],
) -> Result<f32, JetComputeError> {
    if a.len() != b.len() {
        return Err(JetComputeError::InvalidShape(
            "SIMD dot-product inputs have different lengths".to_string(),
        ));
    }
    let value = match backend {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        JetComputeSimdBackend::Avx2 => unsafe { jet_compute_f32_dot_avx2(a, b) },
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        JetComputeSimdBackend::Sse2 => unsafe { jet_compute_f32_dot_sse2(a, b) },
        JetComputeSimdBackend::Scalar => jet_compute_f32_dot_scalar(a, b),
        #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
        _ => jet_compute_f32_dot_scalar(a, b),
    };
    if value.is_finite() {
        Ok(value)
    } else {
        Err(JetComputeError::Arithmetic(
            "f32 SIMD dot product overflowed".to_string(),
        ))
    }
}
// JET_VETTED_UNSAFE_END: jet_compute_cpu_simd

/// CPU-SIMD profile path (#1143): blocked matmul in f32 arithmetic. The dot
/// product uses runtime-dispatched safe intrinsics where available, then an
/// ordered scalar tail. Lane products are reduced in lane order so the SIMD
/// backend preserves the reproducible CPU-oracle reduction contract.
fn jet_compute_matmul_f32_tile(a: &JetTensor, b: &JetTensor) -> Result<JetTensor, JetComputeError> {
    if a.shape.len() != 2 || b.shape.len() != 2 {
        return Err(JetComputeError::RankMismatch(
            "matmul_f32_tile requires rank-2 tensors".to_string(),
        ));
    }
    jet_compute_validate_tensor(a)?;
    jet_compute_validate_tensor(b)?;
    let (m, k) = (a.shape[0], a.shape[1]);
    let (k2, n) = (b.shape[0], b.shape[1]);
    if m < 0 || k < 0 || k2 < 0 || n < 0 {
        return Err(JetComputeError::InvalidShape(
            "matmul_f32_tile dimensions must be non-negative".to_string(),
        ));
    }
    if k != k2 {
        return Err(JetComputeError::RankMismatch(format!(
            "matmul_f32_tile inner dims {} and {} disagree",
            k, k2
        )));
    }
    if a.device == JetComputeDevice::Metal {
        jet_compute_require_same_contract(a, b, "matmul_f32_tile")?;
        let rows = usize::try_from(m).map_err(|_| {
            JetComputeError::InvalidShape("Metal f32 tile row count is too large".to_string())
        })?;
        let inner = usize::try_from(k).map_err(|_| {
            JetComputeError::InvalidShape("Metal f32 tile inner dimension is too large".to_string())
        })?;
        let cols = usize::try_from(n).map_err(|_| {
            JetComputeError::InvalidShape("Metal f32 tile column count is too large".to_string())
        })?;
        let left = jet_compute_metal_values(a, "f32 tile input")?;
        let right = jet_compute_metal_values(b, "f32 tile input")?;
        let data = jet_compute_metal::matmul(&left, &right, rows, inner, cols)?;
        let mut out = jet_compute_metal_result_like(a, vec![m, n], data)?;
        out.last_placement.reason =
            "algorithm=metal-matmul; arithmetic=f32; reduction=ordered; dispatch=metal".to_string();
        return jet_compute_record(
            out,
            &[a, b],
            vec![a.clone(), b.clone()],
            JetComputeTapeRule::MatmulF32Tile,
        );
    }
    let output_shape = vec![m, n];
    let m = usize::try_from(m)
        .map_err(|_| JetComputeError::InvalidShape("f32 tile row count is too large".to_string()))?;
    let k = usize::try_from(k)
        .map_err(|_| JetComputeError::InvalidShape("f32 tile inner dimension is too large".to_string()))?;
    let n = usize::try_from(n)
        .map_err(|_| JetComputeError::InvalidShape("f32 tile column count is too large".to_string()))?;
    let a_values = jet_compute_tensor_values(a)
        .into_iter()
        .map(|value| jet_compute_f32_value(value, "f32 tile input"))
        .collect::<Result<Vec<_>, _>>()?;
    let b_values = jet_compute_tensor_values(b)
        .into_iter()
        .map(|value| jet_compute_f32_value(value, "f32 tile input"))
        .collect::<Result<Vec<_>, _>>()?;
    let packed_b_len = n.checked_mul(k).ok_or_else(|| {
        JetComputeError::InvalidShape("f32 tile packed-B storage length overflow".to_string())
    })?;
    let mut packed_b = vec![0.0_f32; packed_b_len];
    for row in 0..k {
        for column in 0..n {
            packed_b[column * k + row] = b_values[row * n + column];
        }
    }
    let output_len = m.checked_mul(n).ok_or_else(|| {
        JetComputeError::InvalidShape("f32 tile output storage length overflow".to_string())
    })?;
    let backend = jet_compute_simd_backend();
    let mut output = vec![0.0_f64; output_len];
    const TILE: usize = 8;
    for row_tile in (0..m).step_by(TILE) {
        let row_end = row_tile.saturating_add(TILE).min(m);
        for column_tile in (0..n).step_by(TILE) {
            let column_end = column_tile.saturating_add(TILE).min(n);
            for row in row_tile..row_end {
                let left = &a_values[row * k..(row + 1) * k];
                for column in column_tile..column_end {
                    let right = &packed_b[column * k..(column + 1) * k];
                    let value = jet_compute_f32_dot(backend, left, right)?;
                    output[row * n + column] = f64::from(value);
                }
            }
        }
    }
    let mut out = jet_compute_tensor_from_shape(
        output_shape,
        0.0,
        JetComputeDevice::Cpu,
    )?;
    out.data = std::sync::Arc::new(output);
    out.last_placement.profile = CPU_ORACLE_F32_PROFILE.to_string();
    out.last_placement.abilities = CPU_ORACLE_F32_CAPABILITIES
        .iter()
        .map(|ability| (*ability).to_string())
        .collect();
    out.last_placement.reason = format!(
        "algorithm=blocked-matmul; tile={TILE}; arithmetic=f32; reduction=ordered; dispatch={}; vector_width={}; tail=scalar",
        backend.name(),
        backend.width(),
    );
    jet_compute_record(
        out,
        &[a, b],
        vec![a.clone(), b.clone()],
        JetComputeTapeRule::MatmulF32Tile,
    )
}

fn jet_compute_profile_f32_strict() -> String {
    let backend = jet_compute_simd_backend();
    format!(
        "backend={};version={};profile={};algorithm=blocked-matmul;tile=8;dispatch={};vector_width={};tail=scalar;cache={}",
        CPU_ORACLE_BACKEND,
        CPU_ORACLE_VERSION,
        CPU_ORACLE_F32_PROFILE,
        backend.name(),
        backend.width(),
        CPU_ORACLE_CACHE,
    )
}

fn jet_compute_profile_show() -> String {
    jet_compute_profile_f32_strict()
}
