
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
  () => instance.exports.jet_abi_map_string_int_free(0x100, 14),
  "forged map-string-int free",
);
const ptr = instance.exports.jet_abi_map_string_int_alloc(14);
const bytes = new Uint8Array(instance.exports.memory.buffer, ptr, 14);
const view = new DataView(instance.exports.memory.buffer, ptr, 14);
view.setUint32(0, 1, true);
view.setUint32(4, 1, true);
bytes[8] = 120;
view.setUint32(9, 1, true);
bytes[13] = 55;
mustTrap(
  () => instance.exports.jet_export_echo_scores((BigInt(ptr >>> 0) << 32n) | 15n),
  "map-string-int length mismatch",
);
const raw = instance.exports.jet_export_echo_scores(
  (BigInt(ptr >>> 0) << 32n) | 14n,
);
const out = unmarshalAbi(raw, "map-string-int", instance.exports);
if (out.size !== 1 || out.get("x") !== 7n) {
  throw new Error("valid map-string-int allocation did not round-trip");
}
console.log("ok");
