
import { marshalAbi, unmarshalAbi } from "./jet_dom_runtime.js";

function check(ok, message) {
  if (!ok) throw new Error(message);
}

const frees = [];
let cursor = 16;
const wasm = {
  memory: { buffer: new ArrayBuffer(4096) },
  jet_abi_string_alloc(len) {
    const ptr = cursor;
    cursor += len;
    return ptr;
  },
  jet_abi_string_free(ptr, len) {
    frees.push(["string", ptr, len]);
  },
  jet_abi_list_int_alloc(len) {
    const ptr = cursor;
    cursor += len * 8;
    return ptr;
  },
  jet_abi_list_int_free(ptr, len) {
    frees.push(["list-int", ptr, len]);
  },
};

const bounded = {
  memory: { buffer: new ArrayBuffer(64) },
  jet_abi_string_alloc() { return 60; },
  jet_abi_string_free(ptr, len) { frees.push(["string", ptr, len]); },
  jet_abi_list_int_alloc() { return 48; },
  jet_abi_list_int_free(ptr, len) { frees.push(["list-int", ptr, len]); },
};

const RealTextEncoder = globalThis.TextEncoder;
globalThis.TextEncoder = class {
  encode() {
    return { length: 0xfffffff4 };
  }
};
try {
  marshalAbi("x", "string", bounded);
  throw new Error("string length overflow accepted");
} catch (error) {
  check(error instanceof TypeError, "string length overflow was not a TypeError");
}
check(!frees.some((entry) => entry[0] === "string"), "string length overflow allocated");
globalThis.TextEncoder = RealTextEncoder;

try {
  marshalAbi("12345", "string", bounded);
  throw new Error("bounded string write unexpectedly succeeded");
} catch (error) {
  check(error instanceof RangeError, "bounded string write did not fail at memory boundary");
}
check(frees.some((entry) => entry[0] === "string" && entry[1] === 60), "string write failure leaked");

const highBit = {
  memory: { buffer: new ArrayBuffer(64) },
  jet_abi_string_alloc() { return -2147483648; },
  jet_abi_string_free(ptr, len) { frees.push(["string", ptr, len]); },
  jet_abi_list_int_alloc() { return -2147483648; },
  jet_abi_list_int_free(ptr, len) { frees.push(["list-int", ptr, len]); },
};
try {
  marshalAbi("x", "string", highBit);
  throw new Error("high-bit string pointer unexpectedly fit bounded memory");
} catch (error) {
  check(error instanceof RangeError, "high-bit string pointer did not reach the memory boundary");
}
check(
  frees.some((entry) => entry[0] === "string" && entry[1] === 0x80000000),
  "signed string allocation pointer was not freed as u32",
);

try {
  unmarshalAbi((0x80000000n << 32n) | 4n, "string", highBit);
  throw new Error("high-bit string return pointer unexpectedly fit bounded memory");
} catch (error) {
  check(error instanceof RangeError, "high-bit string return pointer did not reach the memory boundary");
}
check(
  frees.some((entry) => entry[0] === "string" && entry[1] === 0x80000000 && entry[2] === 4),
  "high-bit string return pointer was not freed as u32",
);

try {
  unmarshalAbi((0x80000000n << 32n) | 2n, "list-int", highBit);
  throw new Error("high-bit list return pointer unexpectedly fit bounded memory");
} catch (error) {
  check(error instanceof RangeError, "high-bit list return pointer did not reach the memory boundary");
}
check(
  frees.some((entry) => entry[0] === "list-int" && entry[1] === 0x80000000 && entry[2] === 2),
  "high-bit list return pointer was not freed as u32",
);

console.log("ok");
