// A successful run: the raw Wasm edge reports status 0 and an Ok outcome,
// and `jet_main` resolves.
process.on("unhandledRejection", (error) => { throw error; });
const { instantiateWasm, takeWasmError } = await import("./jet_dom_runtime.js");
const { exports } = await instantiateWasm("./app.wasm");
const status = await exports.jet_export_run();
const raw = takeWasmError(exports);
const { jet_main } = await import("./app.js");
const bridge = await jet_main();
console.log(JSON.stringify({ status, raw, bridge: bridge ?? null }));
