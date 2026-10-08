
process.on("unhandledRejection", (error) => { throw error; });
const { instantiateWasm, takeWasmError } = await import("./jet_dom_runtime.js");
const instance = await instantiateWasm("./app.wasm");
const wasm = instance.exports;
const status = wasm.jet_export_run();
const raw = takeWasmError(wasm);
if (status !== 0) throw new Error("fallible Wasm success returned failure status");
if (JSON.stringify(raw) !== '{"tag":"Ok","value":42}') throw new Error("raw Wasm Ok edge changed: " + JSON.stringify(raw));
const { jet_main } = await import("./app.js");
const bridge = await jet_main();
if (bridge !== 42) throw new Error("JS Wasm bridge changed: " + bridge);
console.log(JSON.stringify({ raw, bridge }));
