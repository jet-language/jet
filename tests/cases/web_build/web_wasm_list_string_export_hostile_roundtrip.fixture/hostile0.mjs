
import { instantiateWasm, unmarshalAbi } from "./jet_dom_runtime.js";

function packed(ptr, byteLen) {
  return (BigInt(ptr >>> 0) << 32n) | BigInt(byteLen >>> 0);
}

function mustTrap(call, label) {
  try {
    call();
  } catch (error) {
    if (!(error instanceof WebAssembly.RuntimeError) || error.message !== "unreachable") {
      throw new Error(`${label} raised an unexpected trap: ${error}`);
    }
    return;
  }
  throw new Error(`${label} accepted a malformed ownership token`);
}

const U32_MAX = 0xffff_ffff;
const instance = await instantiateWasm("./app.wasm");

mustTrap(
  () => instance.exports.jet_abi_list_string_free(0x100, 9),
  "forged list-string free (ownership)",
);

const outerPtr = instance.exports.jet_abi_list_string_alloc(9);
const outerBytes = new Uint8Array(instance.exports.memory.buffer, outerPtr, 9);
const outerView = new DataView(instance.exports.memory.buffer, outerPtr, 9);
outerView.setUint32(0, 1, true);
outerView.setUint32(4, 1, true);
outerBytes[8] = 120;
mustTrap(
  () => instance.exports.jet_export_echo_strs(packed(outerPtr, 10)),
  "outer list-string length (ownership)",
);
instance.exports.jet_abi_list_string_free(outerPtr, 9);

const countPtr = instance.exports.jet_abi_list_string_alloc(4);
new DataView(instance.exports.memory.buffer, countPtr, 4).setUint32(0, U32_MAX, true);
const countToken = packed(countPtr, 4);
mustTrap(
  () => instance.exports.jet_export_echo_strs(countToken),
  "embedded list-string count (parser)",
);
mustTrap(
  () => instance.exports.jet_abi_list_string_free(countPtr, 4),
  "embedded list-string count token consumed twice",
);

const lengthPtr = instance.exports.jet_abi_list_string_alloc(8);
const lengthView = new DataView(instance.exports.memory.buffer, lengthPtr, 8);
lengthView.setUint32(0, 1, true);
lengthView.setUint32(4, U32_MAX, true);
const lengthToken = packed(lengthPtr, 8);
mustTrap(
  () => instance.exports.jet_export_echo_strs(lengthToken),
  "embedded list-string element length (parser)",
);
mustTrap(
  () => instance.exports.jet_abi_list_string_free(lengthPtr, 8),
  "embedded list-string length token consumed twice",
);

const validPtr = instance.exports.jet_abi_list_string_alloc(9);
const validBytes = new Uint8Array(instance.exports.memory.buffer, validPtr, 9);
const validView = new DataView(instance.exports.memory.buffer, validPtr, 9);
validView.setUint32(0, 1, true);
validView.setUint32(4, 1, true);
validBytes[8] = 120;
const raw = instance.exports.jet_export_echo_strs(packed(validPtr, 9));
const rawPacked = BigInt(raw);
const rawPtr = Number((rawPacked >> 32n) & 0xffff_ffffn) >>> 0;
const rawLen = Number(rawPacked & 0xffff_ffffn) >>> 0;
const out = unmarshalAbi(raw, "list-string", instance.exports);
if (out.length !== 1 || out[0] !== "x") {
  throw new Error(`valid list-string allocation did not round-trip: ${out}`);
}
mustTrap(
  () => instance.exports.jet_abi_list_string_free(rawPtr, rawLen),
  "valid list-string return freed twice",
);
mustTrap(
  () => instance.exports.jet_abi_list_string_free(validPtr, 9),
  "valid list-string argument retained after transfer",
);
console.log("ok");
