// The program's environment holds only the tz database root, no TZ, so the
// host supplies the zone node's Intl resolves from the process's TZ.
const { instantiateWasm, takeWasmError } = await import("./jet_dom_runtime.js");
const instance = await instantiateWasm("./app.wasm", ["app"], ["JET_TZDB_DIR=" + process.env.JET_TZDB_DIR]);
instance.exports.jet_export_run();
if (takeWasmError(instance.exports)?.tag !== "Ok") throw new Error("unexpected Wasm outcome");
