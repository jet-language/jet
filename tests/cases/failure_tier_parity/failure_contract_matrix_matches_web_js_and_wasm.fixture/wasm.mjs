
const { instantiateWasm, takeWasmError } = await import("./jet_dom_runtime.js");
const instance = await instantiateWasm("./app.wasm");
const status = instance.exports.jet_export_run();
if (status !== 0) throw new Error(`failure matrix Wasm status: ${status}`);
if (takeWasmError(instance.exports)?.tag !== "Ok") throw new Error("failure matrix Wasm stopped");
