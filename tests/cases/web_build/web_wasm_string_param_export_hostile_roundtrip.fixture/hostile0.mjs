
import { instantiateWasm, unmarshalAbi } from "./jet_dom_runtime.js";

function mustTrap(call, label) {
  try {
    call();
  } catch (_) {
    return;
  }
  throw new Error(`${label} accepted an untrusted ownership token`);
}

const instance = await instantiateWasm("./app.wasm");
mustTrap(
  () => instance.exports.jet_abi_string_free(0x100, 1),
  "forged string free",
);
const ptr = instance.exports.jet_abi_string_alloc(1);
new Uint8Array(instance.exports.memory.buffer)[ptr] = 120;
mustTrap(
  () => instance.exports.jet_export_echo_str((BigInt(ptr >>> 0) << 32n) | 2n),
  "string length mismatch",
);
const raw = instance.exports.jet_export_echo_str(
  (BigInt(ptr >>> 0) << 32n) | 1n,
);
if (unmarshalAbi(raw, "string", instance.exports) !== "x") {
  throw new Error("valid registered string allocation did not round-trip");
}
console.log("ok");
