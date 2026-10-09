
import { takeWasmError } from "./jet_dom_runtime.mjs";
import { jet_web_wasm_host_error } from "./runtime_stop.mjs";

const bytes = new TextEncoder().encode("{bad");
const memory = { buffer: new ArrayBuffer(64) };
new Uint8Array(memory.buffer, 8, bytes.length).set(bytes);
let status = 73;
let cleared = 0;
const wasm = {
  memory,
  jet_wasm_error_len: () => bytes.length,
  jet_wasm_error_ptr: () => 8,
  jet_wasm_error_status: () => status,
  jet_wasm_error_clear: () => { cleared += 1; status = 0; },
};
const outcome = takeWasmError(wasm);
const error = jet_web_wasm_host_error(outcome, null, outcome.status);
console.log(JSON.stringify({
  tag: outcome.tag,
  status: outcome.status,
  cleared,
  name: error.name,
  code: error.code,
  errorStatus: error.status,
  exitCode: error.exitCode,
  frame: error.frame,
}));
