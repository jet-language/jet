// Vetted marshalling for the pinned ORT Web WASM C API, not an inference engine.
// ABI authority: microsoft/onnxruntime v1.29.0, onnxruntime/wasm/api.{h,cc}.
// The JS Tensor wrapper omits BF16. Raw OrtCreateTensor/OrtGetTensorData retain
// every declared dtype, including BF16 and 64-bit integer bit patterns.
import { PacketReader, PacketWriter, failure } from "./OnnxRuntimeWeb.js";

let ort; let session = 0; let inputNames = []; let outputNames = [];
const widths = new Map([[1, 4], [2, 1], [3, 1], [4, 2], [5, 2], [6, 4], [7, 8], [9, 1], [10, 2], [11, 8], [12, 4], [13, 8], [16, 2]]);
const resource = message => Object.assign(new Error(message), { kind: 3 });

// Only the already hash-admitted module Blob can be imported below. ORT is
// given wasmBinary, never a runtime or model URL. No model-controlled side
// file, network request, secondary provider, or custom library can be loaded.
const deny = () => { throw new Error("undeclared ONNX runtime I/O is denied"); };
for (const name of ["fetch", "XMLHttpRequest", "WebSocket", "WebTransport", "EventSource", "importScripts", "Worker"]) {
  Object.defineProperty(globalThis, name, { value: deny, writable: false, configurable: false });
}

function lastError(operation) {
  const saved = ort.stackSave();
  try {
    const p = ort.stackAlloc(8);
    ort._OrtGetLastError(p, p + 4);
    const code = ort.HEAP32[p / 4];
    const messagePtr = ort.HEAPU32[p / 4 + 1];
    return Object.assign(new Error(`${operation}: ${messagePtr ? ort.UTF8ToString(messagePtr) : "ORT operation failed"}`), { ortCode: code || 1 });
  } finally { ort.stackRestore(saved); }
}
function check(code, operation) { if (code !== 0) throw lastError(operation); }
function handle(value, operation) { if (!value) throw lastError(operation); return value; }
function alloc(length) {
  if (!Number.isSafeInteger(length) || length < 0 || length > 0xffff_ffff) throw resource("buffer exceeds pinned wasm32 ABI");
  const ptr = ort._malloc(Math.max(1, length));
  if (!ptr) throw resource(`ORT could not allocate ${length} bytes`);
  return ptr;
}
function metadata(index, out) {
  const saved = ort.stackSave(); let name = 0; let info = 0;
  try {
    const p = ort.stackAlloc(8);
    check(ort._OrtGetInputOutputMetadata(session, index, p, p + 4), "OrtGetInputOutputMetadata");
    name = ort.HEAPU32[p / 4]; info = ort.HEAPU32[p / 4 + 1];
    if (!name || !info) throw new Error("ORT returned null endpoint metadata");
    out.text(ort.UTF8ToString(name));
    const dtype = ort.HEAPU32[info / 4]; out.u32(dtype);
    // Non-tensors are reported as a real host fact, then rejected by Rust.
    const rank = dtype === 0 ? 0 : ort.HEAPU32[info / 4 + 1]; out.u32(rank);
    for (let axis = 0; axis < rank; axis++) {
      const symbol = ort.HEAPU32[(info + 8) / 4 + axis];
      const dim = ort.HEAPU32[(info + 8) / 4 + rank + axis];
      out.u64(symbol || dim === 0xffff_ffff ? -1n : BigInt(dim));
    }
    const retained = name; name = 0; return retained;
  } finally {
    if (name) ort._OrtFree(name);
    if (info) ort._OrtFree(info);
    ort.stackRestore(saved);
  }
}

function close() {
  for (const name of [...inputNames, ...outputNames]) ort._OrtFree(name);
  inputNames = []; outputNames = [];
  if (session) ort._OrtReleaseSession(session);
  session = 0;
}

async function open(runtime, graph) {
  if (ort) throw new Error("ORT worker already initialized");
  const moduleUrl = URL.createObjectURL(runtime.module);
  try {
    const factory = (await import(moduleUrl)).default;
    ort = await factory({
      wasmBinary: await runtime.wasm.arrayBuffer(), numThreads: 1,
      locateFile: deny, print: () => {}, printErr: text => console.error(text),
    });
  } finally { URL.revokeObjectURL(moduleUrl); }
  if (ort.PTR_SIZE !== 4) throw new Error("pinned ORT module does not implement the wasm32 ABI");
  ort.asyncInit?.();
  check(ort._OrtInit(1, 2), "OrtInit");
  // The selected archive members are the CPU-only WASM build. Unlike the
  // JSEP/WebGPU builds they cannot register another EP or fall back to one.
  // These options match NativeOnnxProvider (sequential, basic optimization).
  let options = 0; let model = 0;
  try {
    options = handle(ort._OrtCreateSessionOptions(1, true, true, 0, false, 0, 0, 2, 0, 0), "OrtCreateSessionOptions");
    const bytes = new Uint8Array(await graph.arrayBuffer());
    model = alloc(bytes.length); ort.HEAPU8.set(bytes, model);
    session = handle(await ort._OrtCreateSession(model, bytes.length, options), "OrtCreateSession");
    const out = new PacketWriter(); out.u32(0);
    const saved = ort.stackSave();
    try {
      const p = ort.stackAlloc(8);
      check(ort._OrtGetInputOutputCount(session, p, p + 4), "OrtGetInputOutputCount");
      const inputs = ort.HEAPU32[p / 4], outputs = ort.HEAPU32[p / 4 + 1];
      out.u32(inputs);
      for (let i = 0; i < inputs; i++) inputNames.push(metadata(i, out));
      out.u32(outputs);
      for (let i = 0; i < outputs; i++) outputNames.push(metadata(inputs + i, out));
      return out.finish();
    } finally { ort.stackRestore(saved); }
  } catch (error) { close(); throw error; }
  finally { if (model) ort._free(model); if (options) ort._OrtReleaseSessionOptions(options); }
}

async function run(packet) {
  if (!session) throw new Error("ORT worker has no session");
  const reader = new PacketReader(packet);
  const allowance = reader.u64();
  const count = reader.u32();
  if (count !== inputNames.length) throw new Error("host input count differs from session metadata");
  const saved = ort.stackSave(); const allocations = []; const tensors = [];
  let options = 0; let outputsPtr = 0;
  try {
    const inputsPtr = ort.stackAlloc(count * 4), namesPtr = ort.stackAlloc(count * 4);
    const outputNamesPtr = ort.stackAlloc(outputNames.length * 4);
    outputsPtr = ort.stackAlloc(outputNames.length * 4);
    // Initialize all output slots before a failure can reach the finalizer.
    ort.HEAPU32.fill(0, outputsPtr / 4, outputsPtr / 4 + outputNames.length);
    for (let i = 0; i < count; i++) {
      const name = reader.text(), dtype = reader.u32(), rank = reader.u32();
      if (name !== ort.UTF8ToString(inputNames[i])) throw new Error("host input name differs from session metadata");
      // Use heap allocation for untrusted rank; do not grow the Emscripten stack.
      const dims = alloc(rank * 4); allocations.push(dims);
      for (let axis = 0; axis < rank; axis++) {
        const dim = reader.u64();
        if (dim > 0xffff_ffffn) throw resource("tensor dimension exceeds pinned wasm32 ABI");
        ort.HEAPU32[dims / 4 + axis] = Number(dim);
      }
      const bytes = reader.blob(); const data = alloc(bytes.length); allocations.push(data);
      ort.HEAPU8.set(bytes, data);
      const tensor = handle(ort._OrtCreateTensor(dtype, data, bytes.length, dims, rank, 1), "OrtCreateTensor");
      tensors.push(tensor);
      ort.HEAPU32[inputsPtr / 4 + i] = tensor; ort.HEAPU32[namesPtr / 4 + i] = inputNames[i];
    }
    reader.end();
    for (let i = 0; i < outputNames.length; i++) ort.HEAPU32[outputNamesPtr / 4 + i] = outputNames[i];
    options = handle(ort._OrtCreateRunOptions(2, 0, false, 0), "OrtCreateRunOptions");
    check(await ort._OrtRun(session, namesPtr, inputsPtr, count, outputNamesPtr, outputNames.length, outputsPtr, options), "OrtRun");
    const metadata = [];
    let transferred = 0n;
    for (let i = 0; i < outputNames.length; i++) {
      const tensor = handle(ort.HEAPU32[outputsPtr / 4 + i], "OrtRun output");
      const p = ort.stackAlloc(16); let dims = 0;
      try {
        check(ort._OrtGetTensorData(tensor, p, p + 4, p + 8, p + 12), "OrtGetTensorData");
        const dtype = ort.HEAPU32[p / 4], data = ort.HEAPU32[p / 4 + 1];
        dims = ort.HEAPU32[p / 4 + 2]; const rank = ort.HEAPU32[p / 4 + 3];
        const width = widths.get(dtype);
        if (!width) throw Object.assign(new Error(`ORT tensor dtype ${dtype} is outside the declared tensor ABI`), { ortCode: 9 });
        const shape = [];
        let length = BigInt(width);
        for (let axis = 0; axis < rank; axis++) {
          const dim = ort.HEAPU32[dims / 4 + axis]; shape.push(dim); length *= BigInt(dim);
        }
        transferred += length;
        if (length > 0xffff_ffffn || length > BigInt(ort.HEAPU8.length - data)) throw resource("ORT tensor data exceeds the wasm32 heap");
        metadata.push({ name: ort.UTF8ToString(outputNames[i]), dtype, shape, data, length: Number(length) });
      } finally { if (dims) ort._OrtFree(dims); }
    }
    // Report the complete observed output size. Rust applies its canonical
    // output-only and combined-buffer checks; the host does not choose error
    // precedence or copy any tensor data before admission.
    if (transferred > allowance) {
      throw Object.assign(new Error("output transfer exceeds the Rust-computed allowance"), {
        kind: 4, bytes: transferred > 0xffff_ffff_ffff_ffffn ? 0xffff_ffff_ffff_ffffn : transferred, metadata,
      });
    }
    const out = new PacketWriter(); out.u32(0); out.u32(metadata.length);
    for (const tensor of metadata) {
      out.text(tensor.name); out.u32(tensor.dtype); out.u32(tensor.shape.length);
      for (const dim of tensor.shape) out.u64(dim);
      // No further ORT call can grow memory before finish copies these views.
      // Preserve raw bits, including F16/BF16 and all 64-bit integer values.
      out.blob(ort.HEAPU8.subarray(tensor.data, tensor.data + tensor.length));
    }
    return out.finish();
  } finally {
    if (outputsPtr) {
      for (let i = 0; i < outputNames.length; i++) {
        const tensor = ort.HEAPU32[outputsPtr / 4 + i]; if (tensor) ort._OrtReleaseTensor(tensor);
      }
    }
    for (const tensor of tensors) ort._OrtReleaseTensor(tensor);
    for (const ptr of allocations) ort._free(ptr);
    if (options) ort._OrtReleaseRunOptions(options);
    ort.stackRestore(saved);
  }
}

let busy = false;
self.onmessage = async event => {
  if (busy) { const bytes = failure(new Error("concurrent calls on one ORT session")); self.postMessage(bytes, [bytes.buffer]); return; }
  busy = true;
  let bytes;
  try {
    if (event.data.operation === "open") bytes = await open(event.data.runtime, event.data.graph);
    else if (event.data.operation === "run") bytes = await run(event.data.packet);
    else throw new Error("unknown ORT worker operation");
  } catch (error) { bytes = failure(error); }
  finally { busy = false; }
  self.postMessage(bytes, [bytes.buffer]);
};
