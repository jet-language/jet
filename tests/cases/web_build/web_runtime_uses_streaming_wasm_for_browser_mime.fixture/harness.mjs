
const runtime = await import("./jet_dom_runtime.js");
const originalProcess = globalThis.process;
const originalFetch = globalThis.fetch;
const originalStreaming = WebAssembly.instantiateStreaming;
const originalInstantiate = WebAssembly.instantiate;
let streamingCalls = 0;
let fallbackCalls = 0;
WebAssembly.instantiateStreaming = async () => {
  streamingCalls += 1;
  return { instance: { exports: { memory: { buffer: new ArrayBuffer(0) } } } };
};
WebAssembly.instantiate = async () => {
  fallbackCalls += 1;
  throw new Error("array-buffer fallback used");
};
globalThis.process = undefined;
globalThis.fetch = async () => ({
  ok: true,
  headers: { get(name) { return name === "content-type" ? "application/wasm" : null; } },
});
try {
  await runtime.instantiateWasm("app.wasm");
} finally {
  globalThis.process = originalProcess;
  globalThis.fetch = originalFetch;
  WebAssembly.instantiateStreaming = originalStreaming;
  WebAssembly.instantiate = originalInstantiate;
}
if (streamingCalls !== 1) throw new Error(`expected one streaming call, got ${streamingCalls}`);
if (fallbackCalls !== 0) throw new Error(`unexpected fallback call: ${fallbackCalls}`);
console.log("ok");
