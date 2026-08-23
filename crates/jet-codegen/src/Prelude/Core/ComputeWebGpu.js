// D-COMPUTE-BACKEND1=D / I9: browser WebGPU is a host adapter for the one
// compute family. The adapter owns WebGPU's asynchronous queue and buffer ABI;
// placement, precision, operation names, and typed outcomes stay on this
// Prelude boundary. Native hosts keep the typed fail-closed provider in
// CoreLib/Top/Compute.rs.

const JET_WEBGPU_PROFILE = "F32Strict+Reproducible";
const JET_WEBGPU_CAPABILITIES = Object.freeze([
  "ranked-storage",
  "device-buffer",
  "stream",
  "differential-oracle",
]);
const JET_WEBGPU_CPU_PROFILE = "F64Strict+Reproducible";
const JET_WEBGPU_PROFILE_RECEIPT = "backend=webgpu;version=browser;profile=F32Strict+Reproducible;algorithm=webgpu;dispatch=queue;cache=runtime";
const JET_GPU_MAP_READ = 0x0001;
const JET_GPU_COPY_SRC = 0x0004;
const JET_GPU_COPY_DST = 0x0008;
const JET_GPU_UNIFORM = 0x0040;
const JET_GPU_STORAGE = 0x0080;

const jet_webgpu_state = {
  device: null,
  device_promise: null,
  lost: null,
  pipelines: new Map(),
};

function jet_compute_web_ok(value) {
  return { tag: "Ok", values: [value] };
}

function jet_compute_web_err(code, message) {
  return { tag: "Err", values: [{ code, message }] };
}

function jet_compute_web_fail(message, code = "E-COMPUTE-WEBGPU") {
  return jet_compute_web_err(code, message);
}

function jet_compute_web_num(value, label) {
  const number = Number(value);
  if (!Number.isSafeInteger(number) || number < 0) {
    throw new Error(`${label} must be a non-negative safe integer`);
  }
  return number;
}

function jet_compute_web_float(value, label) {
  const number = Number(value);
  if (!Number.isFinite(number)) throw new Error(`${label} must be finite`);
  return number;
}

function jet_compute_web_shape(shape) {
  const out = Array.from(shape, (axis) => jet_compute_web_num(axis, "Tensor dimension"));
  if (!out.length || out.some((axis) => axis < 0)) {
    throw new Error("Tensor shape must contain at least one non-negative dimension");
  }
  return out;
}

function jet_compute_web_numel(shape) {
  return shape.reduce((total, axis) => {
    const next = total * axis;
    if (!Number.isSafeInteger(next)) throw new Error("Tensor storage length overflow");
    return next;
  }, 1);
}

function jet_compute_web_placement(device, profile, reason) {
  if (device === "webgpu") {
    return `Placement(requested=WebGPU, selected=WebGPU, backend=webgpu, version=browser, profile=${profile}, cache=runtime, abilities=[${JET_WEBGPU_CAPABILITIES.map((ability) => `\"${ability}\"`).join(", ")}], reason=${reason})`;
  }
  return `Placement(requested=CPU, selected=CPU, backend=cpu-oracle, version=builtin, profile=${profile}, cache=none, abilities=[\"ranked-storage\"], reason=${reason})`;
}

function jet_compute_web_tensor(shape, values, device = "cpu", profile = JET_WEBGPU_CPU_PROFILE, reason = "policy=explicit") {
  return {
    shape: shape.slice(),
    values: values == null ? null : Array.from(values, Number),
    buffer: null,
    device,
    profile,
    placement: jet_compute_web_placement(device, profile, reason),
    last_transfer: null,
  };
}

function jet_compute_web_device_show(kind) {
  return { cpu: "CPU", metal: "Metal", cuda: "CUDA", vulkan: "Vulkan", webgpu: "WebGPU" }[kind] || kind;
}

function jet_compute_web_device(kind) {
  return { kind, profile: kind === "webgpu" ? JET_WEBGPU_PROFILE : JET_WEBGPU_CPU_PROFILE };
}

function jet_compute_web_same_device(left, right, operation) {
  if (left.device !== right.device || left.profile !== right.profile) {
    throw new Error(`${operation} requires matching device and precision profile`);
  }
}

async function jet_compute_webgpu_device() {
  if (jet_webgpu_state.lost) {
    throw new Error(`WebGPU device was lost: ${jet_webgpu_state.lost}`);
  }
  if (jet_webgpu_state.device) return jet_webgpu_state.device;
  if (typeof navigator === "undefined" || !navigator.gpu) {
    throw new Error("WebGPU device is unavailable; no CPU fallback was selected");
  }
  if (!jet_webgpu_state.device_promise) {
    jet_webgpu_state.device_promise = (async () => {
      const adapter = await navigator.gpu.requestAdapter();
      if (!adapter) throw new Error("WebGPU adapter is unavailable; no CPU fallback was selected");
      const device = await adapter.requestDevice();
      device.lost.then((info) => {
        jet_webgpu_state.lost = info?.message || "device lost";
        jet_webgpu_state.device = null;
      });
      jet_webgpu_state.device = device;
      return device;
    })().catch((error) => {
      jet_webgpu_state.device_promise = null;
      throw error;
    });
  }
  return jet_webgpu_state.device_promise;
}

function jet_compute_webgpu_buffer(device, values) {
  const size = Math.max(4, values.length * 4);
  const buffer = device.createBuffer({
    size: (size + 3) & ~3,
    usage: JET_GPU_STORAGE | JET_GPU_COPY_SRC | JET_GPU_COPY_DST,
    mappedAtCreation: true,
  });
  new Float32Array(buffer.getMappedRange()).set(values);
  buffer.unmap();
  return buffer;
}

function jet_compute_webgpu_params(device, values) {
  const buffer = device.createBuffer({
    size: 32,
    usage: JET_GPU_UNIFORM | JET_GPU_COPY_DST,
  });
  device.queue.writeBuffer(buffer, 0, new Uint32Array(values));
  return buffer;
}

function jet_compute_webgpu_pipeline(device, source, key) {
  let pipeline = jet_webgpu_state.pipelines.get(key);
  if (!pipeline) {
    pipeline = device.createComputePipeline({
      layout: "auto",
      compute: {
        module: device.createShaderModule({ code: source }),
        entryPoint: "main",
      },
    });
    jet_webgpu_state.pipelines.set(key, pipeline);
  }
  return pipeline;
}

async function jet_compute_webgpu_dispatch(source, key, inputs, params, output_len) {
  const device = await jet_compute_webgpu_device();
  const output = device.createBuffer({
    size: Math.max(4, output_len * 4),
    usage: JET_GPU_STORAGE | JET_GPU_COPY_SRC,
  });
  const parameter = jet_compute_webgpu_params(device, params);
  const pipeline = jet_compute_webgpu_pipeline(device, source, key);
  const bindings = inputs.concat([output, parameter]).map((buffer, binding) => ({
    binding,
    resource: { buffer },
  }));
  const group = device.createBindGroup({
    layout: pipeline.getBindGroupLayout(0),
    entries: bindings,
  });
  const encoder = device.createCommandEncoder();
  const pass = encoder.beginComputePass();
  pass.setPipeline(pipeline);
  pass.setBindGroup(0, group);
  pass.dispatchWorkgroups(Math.max(1, Math.ceil(output_len / 64)));
  pass.end();
  device.queue.submit([encoder.finish()]);
  return output;
}

async function jet_compute_webgpu_read(buffer, length) {
  const device = await jet_compute_webgpu_device();
  const readback = device.createBuffer({
    size: Math.max(4, length * 4),
    usage: JET_GPU_COPY_DST | JET_GPU_MAP_READ,
  });
  const encoder = device.createCommandEncoder();
  encoder.copyBufferToBuffer(buffer, 0, readback, 0, Math.max(4, length * 4));
  device.queue.submit([encoder.finish()]);
  await readback.mapAsync(JET_GPU_MAP_READ);
  const values = Array.from(new Float32Array(readback.getMappedRange()).slice(0, length));
  readback.unmap();
  readback.destroy();
  return values;
}

async function jet_compute_web_values(tensor) {
  if (tensor.values != null) return tensor.values.slice();
  tensor.values = await jet_compute_webgpu_read(tensor.buffer, jet_compute_web_numel(tensor.shape));
  return tensor.values.slice();
}

async function jet_compute_web_buffer(tensor) {
  if (tensor.buffer) return tensor.buffer;
  const device = await jet_compute_webgpu_device();
  const values = Float32Array.from(await jet_compute_web_values(tensor));
  if (!values.every(Number.isFinite)) throw new Error("Tensor values must be finite F32 values");
  tensor.buffer = jet_compute_webgpu_buffer(device, values);
  return tensor.buffer;
}

function jet_compute_web_unary_shader(operation) {
  const expression = {
    negate: "-a[i]",
    abs: "abs(a[i])",
    exp: "exp(a[i])",
    log: "log(a[i])",
    sqrt: "sqrt(a[i])",
  }[operation];
  if (!expression) throw new Error(`WebGPU unary operation ${operation} is unsupported`);
  return `struct Params { len: u32, rows: u32, inner: u32, cols: u32, scalar: u32 };
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;
@group(0) @binding(2) var<uniform> p: Params;
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) id: vec3<u32>) {
  let i = id.x; if (i >= p.len) { return; } out[i] = ${expression};
}`;
}

function jet_compute_web_binary_shader(operation) {
  const expression = {
    add: "a[i] + b[i]",
    mul: "a[i] * b[i]",
    sub: "a[i] - b[i]",
    div: "a[i] / b[i]",
    maximum: "max(a[i], b[i])",
    minimum: "min(a[i], b[i])",
  }[operation];
  if (!expression) throw new Error(`WebGPU binary operation ${operation} is unsupported`);
  return `struct Params { len: u32, rows: u32, inner: u32, cols: u32, scalar: u32 };
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> out: array<f32>;
@group(0) @binding(3) var<uniform> p: Params;
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) id: vec3<u32>) {
  let i = id.x; if (i >= p.len) { return; } out[i] = ${expression};
}`;
}

const JET_WEBGPU_MATMUL_SHADER = `struct Params { len: u32, rows: u32, inner: u32, cols: u32, scalar: u32 };
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> out: array<f32>;
@group(0) @binding(3) var<uniform> p: Params;
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) id: vec3<u32>) {
  let index = id.x; if (index >= p.len) { return; }
  let row = index / p.cols; let col = index % p.cols; var total: f32 = 0.0;
  for (var k: u32 = 0u; k < p.inner; k = k + 1u) {
    total = total + a[row * p.inner + k] * b[k * p.cols + col];
  }
  out[index] = total;
}`;

const JET_WEBGPU_SUM_SHADER = `struct Params { len: u32, rows: u32, inner: u32, cols: u32, scalar: u32 };
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;
@group(0) @binding(2) var<uniform> p: Params;
@compute @workgroup_size(1) fn main() { var total: f32 = 0.0;
  for (var i: u32 = 0u; i < p.len; i = i + 1u) { total = total + a[i]; } out[0] = total;
}`;

const JET_WEBGPU_MSE_SHADER = `struct Params { len: u32, rows: u32, inner: u32, cols: u32, scalar: u32 };
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> out: array<f32>;
@group(0) @binding(3) var<uniform> p: Params;
@compute @workgroup_size(1) fn main() { var total: f32 = 0.0;
  for (var i: u32 = 0u; i < p.len; i = i + 1u) { let d = a[i] - b[i]; total = total + d * d; }
  out[0] = total / f32(p.len);
}`;

const JET_WEBGPU_SGD_SHADER = `struct Params { len: u32, rows: u32, inner: u32, cols: u32, scalar: u32 };
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> out: array<f32>;
@group(0) @binding(3) var<uniform> p: Params;
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) id: vec3<u32>) {
  let i = id.x; if (i >= p.len) { return; } out[i] = a[i] - bitcast<f32>(p.scalar) * b[i];
}`;

function jet_compute_web_f32_values(values, label) {
  const out = Float32Array.from(values);
  if (!Array.from(out).every(Number.isFinite)) throw new Error(`${label} exceeds finite F32 precision`);
  return out;
}

async function jet_compute_web_gpu_unary(operation, tensor) {
  const buffer = await jet_compute_web_buffer(tensor);
  const length = jet_compute_web_numel(tensor.shape);
  const output = await jet_compute_webgpu_dispatch(
    jet_compute_web_unary_shader(operation), `unary:${operation}`, [buffer], [length, 0, 0, 0, 0], length,
  );
  const result = jet_compute_web_tensor(tensor.shape, null, "webgpu", JET_WEBGPU_PROFILE, `algorithm=webgpu-${operation};arithmetic=f32`);
  result.buffer = output;
  return result;
}

async function jet_compute_web_gpu_binary(operation, left, right, shape, left_values, right_values) {
  const device = await jet_compute_webgpu_device();
  const left_buffer = left_values ? jet_compute_webgpu_buffer(device, jet_compute_web_f32_values(left_values, "left input")) : await jet_compute_web_buffer(left);
  const right_buffer = right_values ? jet_compute_webgpu_buffer(device, jet_compute_web_f32_values(right_values, "right input")) : await jet_compute_web_buffer(right);
  const length = jet_compute_web_numel(shape);
  const output = await jet_compute_webgpu_dispatch(
    jet_compute_web_binary_shader(operation), `binary:${operation}`, [left_buffer, right_buffer], [length, 0, 0, 0, 0], length,
  );
  const result = jet_compute_web_tensor(shape, null, "webgpu", JET_WEBGPU_PROFILE, `algorithm=webgpu-${operation};arithmetic=f32`);
  result.buffer = output;
  return result;
}

async function jet_compute_web_gpu_matmul(left, right, rows, inner, cols) {
  const output_length = rows * cols;
  const output = await jet_compute_webgpu_dispatch(
    JET_WEBGPU_MATMUL_SHADER,
    "matmul",
    [await jet_compute_web_buffer(left), await jet_compute_web_buffer(right)],
    [output_length, rows, inner, cols, 0],
    output_length,
  );
  const result = jet_compute_web_tensor([rows, cols], null, "webgpu", JET_WEBGPU_PROFILE, "algorithm=webgpu-matmul;arithmetic=f32;reduction=ordered");
  result.buffer = output;
  return result;
}

function jet_compute_web_broadcast_shape(left, right) {
  const length = Math.max(left.length, right.length);
  const shape = [];
  for (let i = 1; i <= length; i += 1) {
    const a = left[left.length - i] ?? 1;
    const b = right[right.length - i] ?? 1;
    if (a !== b && a !== 1 && b !== 1) throw new Error("broadcast dimensions disagree");
    shape.unshift(Math.max(a, b));
  }
  return shape;
}

function jet_compute_web_broadcast_values(values, source_shape, target_shape) {
  if (source_shape.length === target_shape.length && source_shape.every((v, i) => v === target_shape[i])) return values.slice();
  const out = new Array(jet_compute_web_numel(target_shape));
  const source_strides = [];
  let stride = 1;
  for (let i = source_shape.length - 1; i >= 0; i -= 1) { source_strides[i] = stride; stride *= source_shape[i]; }
  for (let flat = 0; flat < out.length; flat += 1) {
    let rest = flat;
    let source_flat = 0;
    for (let i = target_shape.length - 1; i >= 0; i -= 1) {
      const coordinate = rest % target_shape[i]; rest = Math.floor(rest / target_shape[i]);
      const source_axis = i - (target_shape.length - source_shape.length);
      if (source_axis >= 0 && source_shape[source_axis] !== 1) source_flat += coordinate * source_strides[source_axis];
    }
    out[flat] = values[source_flat];
  }
  return out;
}

async function jet_compute_web_binary(operation, left, right) {
  jet_compute_web_same_device(left, right, operation);
  const shape = jet_compute_web_broadcast_shape(left.shape, right.shape);
  if (left.device === "webgpu") {
    const left_values = jet_compute_web_broadcast_values(await jet_compute_web_values(left), left.shape, shape);
    const right_values = jet_compute_web_broadcast_values(await jet_compute_web_values(right), right.shape, shape);
    return jet_compute_web_gpu_binary(operation, left, right, shape, left_values, right_values);
  }
  const left_values = jet_compute_web_broadcast_values(await jet_compute_web_values(left), left.shape, shape);
  const right_values = jet_compute_web_broadcast_values(await jet_compute_web_values(right), right.shape, shape);
  const out = left_values.map((value, index) => ({
    add: value + right_values[index], mul: value * right_values[index], sub: value - right_values[index],
    div: value / right_values[index], maximum: Math.max(value, right_values[index]), minimum: Math.min(value, right_values[index]),
  }[operation]));
  return jet_compute_web_tensor(shape, out, "cpu", left.profile, "policy=explicit;selected=cpu;ability=cpu-oracle");
}

async function jet_compute_web_unary(operation, tensor) {
  if (tensor.device === "webgpu") return jet_compute_web_gpu_unary(operation, tensor);
  const values = await jet_compute_web_values(tensor);
  const f = { negate: (x) => -x, abs: Math.abs, exp: Math.exp, log: Math.log, sqrt: Math.sqrt }[operation];
  if (!f) throw new Error(`unary operation ${operation} is unsupported`);
  return jet_compute_web_tensor(tensor.shape, values.map(f), "cpu", tensor.profile, "policy=explicit;selected=cpu;ability=cpu-oracle");
}

async function jet_compute_web_matmul(left, right, f32_tile = false) {
  jet_compute_web_same_device(left, right, "matmul");
  if (left.shape.length !== 2 || right.shape.length !== 2 || left.shape[1] !== right.shape[0]) throw new Error("matmul dimensions disagree");
  const rows = left.shape[0]; const inner = left.shape[1]; const cols = right.shape[1];
  if (left.device === "webgpu") return jet_compute_web_gpu_matmul(left, right, rows, inner, cols);
  const a = f32_tile ? jet_compute_web_f32_values(await jet_compute_web_values(left), "matmul input") : await jet_compute_web_values(left);
  const b = f32_tile ? jet_compute_web_f32_values(await jet_compute_web_values(right), "matmul input") : await jet_compute_web_values(right);
  const out = new Array(rows * cols).fill(0);
  for (let row = 0; row < rows; row += 1) for (let col = 0; col < cols; col += 1) {
    let total = 0;
    for (let k = 0; k < inner; k += 1) total = f32_tile ? Math.fround(Math.fround(total) + Math.fround(a[row * inner + k] * b[k * cols + col])) : total + a[row * inner + k] * b[k * cols + col];
    out[row * cols + col] = total;
  }
  return jet_compute_web_tensor([rows, cols], f32_tile ? jet_compute_web_f32_values(out, "matmul output") : out, "cpu", f32_tile ? JET_WEBGPU_PROFILE : left.profile, f32_tile ? "algorithm=blocked-matmul;arithmetic=f32;reduction=ordered;dispatch=scalar" : "policy=explicit;selected=cpu;ability=cpu-oracle");
}

async function jet_compute_web_upload(tensor, device) {
  if (device.kind === "cpu") return jet_compute_web_tensor(tensor.shape, await jet_compute_web_values(tensor), "cpu", tensor.profile, "policy=explicit;selected=cpu;ability=cpu-oracle");
  if (device.kind !== "webgpu") throw new Error(`${device.kind} backend is unavailable in the browser WebGPU provider`);
  if (tensor.profile !== JET_WEBGPU_PROFILE) throw new Error("WebGPU backend supports only F32Strict+Reproducible; create an F32 Tensor first");
  await jet_compute_webgpu_device();
  const result = jet_compute_web_tensor(tensor.shape, null, "webgpu", JET_WEBGPU_PROFILE, "policy=explicit;selected=webgpu;ability=webgpu.f32");
  result.buffer = jet_compute_webgpu_buffer(await jet_compute_webgpu_device(), jet_compute_web_f32_values(await jet_compute_web_values(tensor), "WebGPU transfer"));
  return result;
}

async function jet_compute_web_transfer(tensor, device) {
  const from = tensor.device;
  const result = await jet_compute_web_upload(tensor, device);
  const bytes = from === result.device ? 0 : jet_compute_web_numel(tensor.shape) * (tensor.profile === JET_WEBGPU_PROFILE ? 4 : 8);
  result.last_transfer = `Transfer(from=${jet_compute_web_device_show(from)}, to=${jet_compute_web_device_show(result.device)}, bytes=${bytes}, fallback=${from === result.device ? "none" : "not-applicable"})`;
  return result;
}

async function jet_compute_web_sum(tensor) {
  if (tensor.device === "webgpu") {
    const output = await jet_compute_webgpu_dispatch(JET_WEBGPU_SUM_SHADER, "sum", [await jet_compute_web_buffer(tensor)], [jet_compute_web_numel(tensor.shape), 0, 0, 0, 0], 1);
    const result = jet_compute_web_tensor([1], null, "webgpu", JET_WEBGPU_PROFILE, "algorithm=webgpu-sum;arithmetic=f32;reduction=ordered");
    result.buffer = output;
    return result;
  }
  return jet_compute_web_tensor([1], [(await jet_compute_web_values(tensor)).reduce((a, b) => a + b, 0)], "cpu", tensor.profile, "policy=explicit;selected=cpu;ability=cpu-oracle");
}

async function jet_compute_web_mse(left, right) {
  jet_compute_web_same_device(left, right, "mse_loss");
  if (left.shape.length !== right.shape.length || left.shape.some((v, i) => v !== right.shape[i])) throw new Error("mse_loss requires matching shapes");
  const length = jet_compute_web_numel(left.shape);
  if (!length) throw new Error("mse_loss requires non-empty tensors");
  if (left.device === "webgpu") {
    const output = await jet_compute_webgpu_dispatch(JET_WEBGPU_MSE_SHADER, "mse", [await jet_compute_web_buffer(left), await jet_compute_web_buffer(right)], [length, 0, 0, 0, 0], 1);
    const result = jet_compute_web_tensor([1], null, "webgpu", JET_WEBGPU_PROFILE, "algorithm=webgpu-mse;arithmetic=f32;reduction=ordered");
    result.buffer = output;
    return result;
  }
  const a = await jet_compute_web_values(left); const b = await jet_compute_web_values(right);
  return jet_compute_web_tensor([1], [a.reduce((sum, value, i) => sum + (value - b[i]) ** 2, 0) / length], "cpu", left.profile, "policy=explicit;selected=cpu;ability=cpu-oracle");
}

async function jet_compute_web_sgd(parameter, gradient, rate) {
  jet_compute_web_same_device(parameter, gradient, "sgd_step");
  const scalar = jet_compute_web_float(rate, "sgd_step learning rate");
  if (!Number.isFinite(scalar) || scalar < 0) throw new Error("sgd_step learning rate must be finite and non-negative");
  if (parameter.shape.length !== gradient.shape.length || parameter.shape.some((v, i) => v !== gradient.shape[i])) throw new Error("sgd_step requires matching shapes");
  if (parameter.device === "webgpu") {
    const bits = new ArrayBuffer(4); new DataView(bits).setFloat32(0, scalar, true);
    const output = await jet_compute_webgpu_dispatch(JET_WEBGPU_SGD_SHADER, "sgd", [await jet_compute_web_buffer(parameter), await jet_compute_web_buffer(gradient)], [jet_compute_web_numel(parameter.shape), 0, 0, 0, new DataView(bits).getUint32(0, true)], jet_compute_web_numel(parameter.shape));
    const result = jet_compute_web_tensor(parameter.shape, null, "webgpu", JET_WEBGPU_PROFILE, "algorithm=webgpu-sgd;arithmetic=f32");
    result.buffer = output;
    return result;
  }
  const a = await jet_compute_web_values(parameter); const b = await jet_compute_web_values(gradient);
  return jet_compute_web_tensor(parameter.shape, a.map((value, i) => value - scalar * b[i]), "cpu", parameter.profile, "policy=explicit;selected=cpu;ability=cpu-oracle");
}

async function jet_compute_web_set(tensor, indices, value) {
  const values = await jet_compute_web_values(tensor);
  const coords = Array.from(indices, (index) => jet_compute_web_num(index, "Tensor index"));
  if (coords.length !== tensor.shape.length || coords.some((index, axis) => index >= tensor.shape[axis])) throw new Error("Tensor index is out of bounds");
  let flat = 0;
  for (let axis = 0; axis < coords.length; axis += 1) flat = flat * tensor.shape[axis] + coords[axis];
  values[flat] = jet_compute_web_float(value, "Tensor value");
  tensor.values = values;
  if (tensor.device === "webgpu") tensor.buffer = jet_compute_webgpu_buffer(await jet_compute_webgpu_device(), jet_compute_web_f32_values(values, "Tensor value"));
  return jet_compute_web_ok(undefined);
}

function jet_compute_web_shape_result(tensor, shape, reason = "algorithm=reshape;metadata-only") {
  const result = jet_compute_web_tensor(shape, tensor.values, tensor.device, tensor.profile, reason);
  result.buffer = tensor.buffer;
  return result;
}

async function jet_compute_web_call(method, args) {
  try {
    switch (method) {
      case "zeros": return jet_compute_web_ok(jet_compute_web_tensor(jet_compute_web_shape(args[0]), new Array(jet_compute_web_numel(jet_compute_web_shape(args[0]))).fill(0)));
      case "ones": return jet_compute_web_ok(jet_compute_web_tensor(jet_compute_web_shape(args[0]), new Array(jet_compute_web_numel(jet_compute_web_shape(args[0]))).fill(1)));
      case "full": { const shape = jet_compute_web_shape(args[0]); return jet_compute_web_ok(jet_compute_web_tensor(shape, new Array(jet_compute_web_numel(shape)).fill(jet_compute_web_float(args[1], "Tensor value")))); }
      case "from_list": return jet_compute_web_ok(jet_compute_web_tensor([args[0].length], args[0].map((value) => jet_compute_web_float(value, "Tensor value"))));
      case "matrix": { const rows = jet_compute_web_num(args[0], "Matrix rows"); const cols = jet_compute_web_num(args[1], "Matrix cols"); return jet_compute_web_ok(jet_compute_web_tensor([rows, cols], new Array(rows * cols).fill(jet_compute_web_float(args[2], "Tensor value")))); }
      case "vec": { const len = jet_compute_web_num(args[0], "Vec length"); return jet_compute_web_ok(jet_compute_web_tensor([len], new Array(len).fill(jet_compute_web_float(args[1], "Tensor value")))); }
      case "add": case "mul": case "sub": case "div": case "maximum": case "minimum": return jet_compute_web_ok(await jet_compute_web_binary(method, args[0], args[1]));
      case "negate": case "abs": case "exp": case "log": case "sqrt": return jet_compute_web_ok(await jet_compute_web_unary(method, args[0]));
      case "matmul": return jet_compute_web_ok(await jet_compute_web_matmul(args[0], args[1]));
      case "matmul_f32_tile": return jet_compute_web_ok(await jet_compute_web_matmul(args[0], args[1], true));
      case "reshape": { const shape = jet_compute_web_shape(args[1]); if (jet_compute_web_numel(shape) !== jet_compute_web_numel(args[0].shape)) throw new Error("reshape changes Tensor element count"); return jet_compute_web_ok(jet_compute_web_shape_result(args[0], shape)); }
      case "shape": return args[0].shape.map(BigInt);
      case "rank": return BigInt(args[0].shape.length);
      case "numel": return BigInt(jet_compute_web_numel(args[0].shape));
      case "to_list": return jet_compute_web_values(args[0]);
      case "get": { const values = await jet_compute_web_values(args[0]); const indices = Array.from(args[1], (v) => jet_compute_web_num(v, "Tensor index")); if (indices.length !== args[0].shape.length || indices.some((index, axis) => index >= args[0].shape[axis])) throw new Error("Tensor index is out of bounds"); let flat = 0; for (let axis = 0; axis < indices.length; axis += 1) flat = flat * args[0].shape[axis] + indices[axis]; return jet_compute_web_ok(values[flat]); }
      case "set": return jet_compute_web_set(args[0], args[1], args[2]);
      case "device": return jet_compute_web_device_show(args[0].device);
      case "placement": return args[0].placement;
      case "device_cpu": return jet_compute_web_device("cpu");
      case "device_auto": return jet_compute_web_device("cpu");
      case "device_webgpu": return jet_compute_web_device("webgpu");
      case "device_metal": return jet_compute_web_device("metal");
      case "device_cuda": return jet_compute_web_device("cuda");
      case "device_vulkan": return jet_compute_web_device("vulkan");
      case "on_device": return jet_compute_web_ok(await jet_compute_web_upload(args[0], args[1]));
      case "transfer": return jet_compute_web_ok(await jet_compute_web_transfer(args[0], args[1]));
      case "sum_axis": { const axis = jet_compute_web_num(args[1], "reduction axis"); if (args[0].shape.length !== 1 || axis !== 0) throw new Error("WebGPU sum_axis currently supports ranked vector reduction only"); return jet_compute_web_ok(await jet_compute_web_sum(args[0])); }
      case "mse_loss": return jet_compute_web_ok(await jet_compute_web_mse(args[0], args[1]));
      case "sgd_step": return jet_compute_web_ok(await jet_compute_web_sgd(args[0], args[1], args[2]));
      case "stream_new": return { device: "cpu", profile: JET_WEBGPU_CPU_PROFILE, id: "cpu-stream" };
      case "stream_new_on": { const device = args[0]; if (device.kind !== "cpu" && device.kind !== "webgpu") throw new Error(`${device.kind} stream is unavailable in the browser WebGPU provider`); if (device.kind === "webgpu") await jet_compute_webgpu_device(); return jet_compute_web_ok({ device: device.kind, profile: device.profile, id: `${device.kind}-stream` }); }
      case "stream_sync": if (args[0].device === "webgpu") await (await jet_compute_webgpu_device()).queue.onSubmittedWorkDone(); return jet_compute_web_ok(undefined);
      case "stream_show": return `ComputeStream(device=${jet_compute_web_device_show(args[0].device)})`;
      case "profile_f32_strict": return JET_WEBGPU_PROFILE_RECEIPT;
      case "profile_show": return JET_WEBGPU_PROFILE_RECEIPT;
      case "kernel_bounds_ok": { const shape = args[0].map((v) => jet_compute_web_num(v, "kernel shape")); const index = args[1].map((v) => jet_compute_web_num(v, "kernel index")); return jet_compute_web_ok(shape.length === index.length && index.every((v, i) => v < shape[i])); }
      case "transfer_show": return args[0].last_transfer || args[0].placement;
      case "broadcast_to": case "transpose": case "eye": case "det": case "inv": case "solve": case "fft": case "serialize": case "deserialize": case "to_sparse": case "sparse_nnz": case "sparse_mv": case "sparse_show": case "gradient": case "value_and_gradient": case "jvp": case "vjp": throw new Error(`${method} is not in the browser WebGPU F32 operation subset`);
      default: throw new Error(`core.compute operation ${method} is unsupported by the browser WebGPU provider`);
    }
  } catch (error) {
    return jet_compute_web_fail(error?.message || String(error));
  }
}
