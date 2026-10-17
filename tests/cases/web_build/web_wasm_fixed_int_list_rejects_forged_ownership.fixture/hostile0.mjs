
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
  () => instance.exports.jet_abi_list_i64_free(0x100, 1),
  "forged fixed-list free",
);
const ptr = instance.exports.jet_abi_list_i64_alloc(2);
const values = new BigInt64Array(instance.exports.memory.buffer, ptr, 2);
values[0] = 7n;
values[1] = -8n;
mustTrap(
  () => instance.exports.jet_export_echo_fixed((BigInt(ptr >>> 0) << 32n) | 3n),
  "fixed-list length mismatch",
);
const raw = instance.exports.jet_export_echo_fixed(
  (BigInt(ptr >>> 0) << 32n) | 2n,
);
const out = unmarshalAbi(raw, "list-i64", instance.exports);
if (out.length !== 2 || out[0] !== 7 || out[1] !== -8) {
  throw new Error(`valid fixed-list allocation did not round-trip: ${out}`);
}
console.log("ok");
