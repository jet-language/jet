// Browser transport for the runtime-owned OnnxRuntimeProvider. Package
// semantics and errors stay in Rust; runtime bytes are supplied by the
// embedding and are never fetched.

import { createOnnxRuntimeWebHost, PacketReader, PacketWriter } from "../../../jet-rt/src/model/OnnxRuntimeWeb.js";
function bytes(value) {
  if (value instanceof Uint8Array) return value;
  if (ArrayBuffer.isView(value)) return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
  return new Uint8Array(value);
}

function writeArtifact(p, artifact) {
  p.text(artifact.path);
  p.text(artifact.sha256);
}

function writeOptionalU64(p, value) {
  p.u64(value == null ? ((1n << 64n) - 1n) : value);
}

function writeTensor(p, tensor) {
  p.text(tensor.name);
  p.text(tensor.dtype);
  p.u32(tensor.shape.length);
  for (const dimension of tensor.shape) {
    if (typeof dimension === "number") {
      p.u32(0);
      p.u64(dimension);
    } else {
      p.u32(1);
      p.text(dimension.name);
      p.u64(dimension.min);
      p.u64(dimension.max);
    }
  }
}

function writeDescriptor(p, descriptor, output) {
  if (descriptor.output && descriptor.output !== output) {
    throw new Error("model descriptor output does not match request output");
  }
  p.text(descriptor.package);
  if (descriptor.signatureName == null) {
    p.u32(0);
  } else {
    p.u32(1);
    p.text(descriptor.signatureName);
  }
  p.text(descriptor.packageVersion);
  p.text(descriptor.license);
  writeArtifact(p, descriptor.graph);
  writeArtifact(p, descriptor.weights);
  writeArtifact(p, descriptor.tokenizer);
  if (descriptor.adapter == null) {
    p.u32(0);
  } else {
    p.u32(1);
    writeArtifact(p, descriptor.adapter);
  }
  p.text(descriptor.preprocessing);
  p.text(descriptor.pooling);
  p.text(descriptor.normalization);
  p.text(descriptor.outputMeaning);
  p.text(descriptor.metric);
  p.text(descriptor.contract.provider);
  p.u32(Number(Boolean(descriptor.contract.customOperators)));
  for (const tensor of [descriptor.contract.inputs, descriptor.contract.outputs]) {
    p.u32(tensor.length);
    for (const item of tensor) writeTensor(p, item);
  }
  writeOptionalU64(p, descriptor.contract.maxContext);
  writeOptionalU64(p, descriptor.contract.maxBatch);
  writeOptionalU64(p, descriptor.contract.maxBufferBytes);
}

function i64Bytes(values) {
  const out = new Uint8Array(values.length * 8);
  const view = new DataView(out.buffer);
  values.forEach((value, index) => view.setBigInt64(index * 8, BigInt(value), true));
  return out;
}

// Build an offline request after a host has parsed the package source with
// ModelPackage::descriptor_from_source and loaded staged artifact bytes.
export function prepareStagedRequest({ manifest, corpus, descriptor, artifacts, document = corpus.documents[0], outputPolicy = "exact", warmRuns = 1, resumeAfterCancel = false }) {
  if (manifest.package !== corpus.model || manifest.revision !== corpus.revision) {
    throw new Error("staged package and corpus revisions differ");
  }
  if (manifest.tokenizer.sha256 !== corpus.tokenizer_sha256) {
    throw new Error("staged tokenizer hash differs from corpus");
  }
  if (!document || typeof document.text !== "string"
      || document.input_ids.length !== corpus.sequence_length
      || document.attention_mask.length !== corpus.sequence_length
      || document.token_type_ids.length !== corpus.sequence_length) {
    throw new Error("staged corpus document has the wrong sequence length");
  }
  if (!descriptor
      || descriptor.package !== manifest.package
      || descriptor.packageVersion !== manifest.revision
      || descriptor.tokenizer?.sha256 !== manifest.tokenizer.sha256
      || descriptor.output !== manifest.output
      || descriptor.signatureName !== manifest.signatureName) {
    throw new Error("host descriptor does not match staged package source");
  }
  const inputs = [
    ["input_ids", document.input_ids],
    ["attention_mask", document.attention_mask],
    ["token_type_ids", document.token_type_ids],
  ].map(([name, values]) => ({
    name,
    dtype: "i64",
    shape: [1, values.length],
    bytes: i64Bytes(values),
  }));
  return encodeRequest({
    output: manifest.output,
    descriptor,
    operators: manifest.operators,
    artifacts,
    inputs,
    documents: [document.text],
    outputPolicy,
    warmRuns,
    resumeAfterCancel,
  });
}

export function encodeRequest({ output, descriptor, operators, artifacts, inputs, documents = [], outputPolicy = "exact", warmRuns = 1, resumeAfterCancel = false }) {
  const p = new PacketWriter(); p.text(output); writeDescriptor(p, descriptor, output); p.text(outputPolicy);
  p.u32(warmRuns); p.u32(Number(resumeAfterCancel));
  p.u32(operators.length); for (const operator of operators) p.text(operator);
  const files = artifacts instanceof Map ? [...artifacts] : Object.entries(artifacts);
  p.u32(files.length); for (const [path, data] of files) { p.text(path); p.blob(bytes(data)); }
  p.u32(inputs.length);
  for (const input of inputs) {
    p.text(input.name); p.text(input.dtype); p.u32(input.shape.length);
    for (const dim of input.shape) p.u64(dim);
    p.blob(bytes(input.bytes));
  }
  if (documents.length === 0) {
    p.u32(0);
  } else {
    p.u32(1); p.u32(documents.length);
    for (const document of documents) p.text(document);
  }
  return p.finish();
}

export function decodeResult(bytes) {
  const p = new PacketReader(bytes);
  if (p.u32() !== 0) { const error = { code: p.text(), detail: p.text() }; p.end(); return { error }; }
  const identity = p.text(), provenance = p.text(), resumed = p.u32() !== 0;
  const outputs = [];
  for (let count = p.u32(); count > 0; count--) {
    const name = p.text(), dtype = p.text(), shape = [];
    for (let rank = p.u32(); rank > 0; rank--) shape.push(p.u64());
    outputs.push({ name, dtype, shape, bytes: p.blob().slice() });
  }
  const measurement = p.text(); p.end();
  return { identity, provenance, resumed, outputs, measurement };
}

export async function runBrowser({ wasmBytes, runtimeArchive, request, signal, onRun }) {
  const host = createOnnxRuntimeWebHost(runtimeArchive);
  let instance; let finished;
  const ready = new Promise(resolve => { finished = resolve; });
  const imports = { ...host.imports, jet_onnx_proof: {
    schedule() { queueMicrotask(() => instance.exports.proof_poll()); },
    finished() { finished(); },
  } };
  const start = imports.jet_onnx_web.start;
  imports.jet_onnx_web.start = (...args) => {
    start(...args);
    if (args[2] === 1) onRun?.();
  };
  const cancel = () => instance.exports.proof_cancel();
  try {
    ({ instance } = await WebAssembly.instantiate(wasmBytes, imports));
    host.bind(instance.exports);
    const packet = request instanceof Uint8Array ? request : encodeRequest(request);
    const ptr = instance.exports.proof_alloc(packet.length) >>> 0;
    if (!ptr) throw new Error("proof request allocation failed");
    new Uint8Array(instance.exports.memory.buffer, ptr, packet.length).set(packet);
    instance.exports.proof_start();
    signal?.addEventListener("abort", cancel, { once: true });
    if (signal?.aborted) cancel();
    await ready;
    const result = new Uint8Array(instance.exports.memory.buffer, instance.exports.proof_result_ptr() >>> 0, instance.exports.proof_result_len() >>> 0).slice();
    return { packet: result, result: decodeResult(result) };
  } finally {
    signal?.removeEventListener("abort", cancel);
    instance?.exports.proof_drop();
    host.dispose();
  }
}

export function compareExact(native, web) {
  if (native.error || web.error) throw new Error("a provider returned a declared failure; parity is not proved");
  if (native.identity !== web.identity) throw new Error("model identity differs across tiers");
  if (!native.provenance.includes("output-policy=exact\n") || !web.provenance.includes("output-policy=exact\n")) throw new Error("exact comparison requires exact output policy on both tiers");
  if (native.outputs.length !== web.outputs.length) throw new Error("output count differs across tiers");
  for (let i = 0; i < native.outputs.length; i++) {
    const a = native.outputs[i], b = web.outputs[i];
    if (a.name !== b.name || a.dtype !== b.dtype || a.shape.join(",") !== b.shape.join(",")) throw new Error("output signature differs across tiers");
    if (a.bytes.length !== b.bytes.length || a.bytes.some((value, j) => value !== b.bytes[j])) throw new Error(`output bits differ for ${a.name}`);
  }
  return { identity: native.identity, outputs: native.outputs.map(({ name, dtype, shape }) => ({ name, dtype, shape })) };
}
