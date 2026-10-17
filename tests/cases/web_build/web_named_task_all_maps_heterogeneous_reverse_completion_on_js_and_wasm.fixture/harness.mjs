
const { instantiateWasm, takeWasmError } = await import("./jet_dom_runtime.js");
const instance = await instantiateWasm("./app.wasm");
instance.exports.jet_export_run();
if (takeWasmError(instance.exports)?.tag !== "Ok") throw new Error("unexpected Wasm outcome");
