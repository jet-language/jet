
import { marshalAbi, unmarshalAbi } from "./jet_dom_runtime.js";

function check(ok, message) {
  if (!ok) throw new Error(message);
}

let cursor = 16;
const frees = [];
const wasm = {
  memory: { buffer: new ArrayBuffer(4096) },
  jet_abi_map_string_int_alloc(byteLen) {
    const ptr = cursor;
    cursor += byteLen;
    return ptr;
  },
  jet_abi_map_string_int_free(ptr, byteLen) {
    frees.push([ptr, byteLen]);
  },
};
const limits = new Map([
  ["min", -9223372036854775808n],
  ["max", 9223372036854775807n],
  ["above-safe", 9007199254740993n],
  ["below-safe", -9007199254740993n],
  ["small", 7n],
]);
const packed = marshalAbi(limits, "map-string-int", wasm);
const decoded = unmarshalAbi(packed, "map-string-int", wasm);
for (const [key, value] of limits) {
  check(typeof decoded.get(key) === "bigint", `${key} was not BigInt`);
  check(decoded.get(key) === value, `${key} changed`);
}
const repacked = marshalAbi(decoded, "map-string-int", wasm);
const decodedTwice = unmarshalAbi(repacked, "map-string-int", wasm);
for (const [key, value] of limits) check(decodedTwice.get(key) === value, `${key} changed on re-marshal`);
try {
  marshalAbi(new Map([["number", 1]]), "map-string-int", wasm);
  throw new Error("Number-backed Int accepted");
} catch (error) {
  check(error instanceof TypeError, "Number-backed Int was not rejected");
}

let allocations = 0;
const bounded = {
  memory: { buffer: new ArrayBuffer(64) },
  jet_abi_map_string_int_alloc() { allocations += 1; return 60; },
  jet_abi_map_string_int_free(ptr, byteLen) { frees.push([ptr, byteLen]); },
};
class HiddenEntryMap extends Map {
  get size() { return 0; }
  *[Symbol.iterator]() { yield ["hidden", 1n]; }
}
try {
  marshalAbi(new HiddenEntryMap(), "map-string-int", bounded);
  throw new Error("size-zero iterator entry accepted");
} catch (error) {
  check(error instanceof TypeError && error.message.includes("size/iterator mismatch"), "size-zero iterator mismatch was not rejected");
}
check(allocations === 0, "size-zero iterator mismatch allocated");

class MissingEntryMap extends Map {
  get size() { return 2; }
  *[Symbol.iterator]() { yield ["visible", 1n]; }
}
try {
  marshalAbi(new MissingEntryMap(), "map-string-int", bounded);
  throw new Error("short iterator accepted");
} catch (error) {
  check(error instanceof TypeError && error.message.includes("size/iterator mismatch"), "short iterator mismatch was not rejected");
}
check(allocations === 0, "short iterator mismatch allocated");

class HugeMap extends Map { get size() { return 0x1_0000_0000; } }
try {
  marshalAbi(new HugeMap([["x", 1n]]), "map-string-int", bounded);
  throw new Error("count overflow accepted");
} catch (error) {
  check(error instanceof RangeError, "count overflow was not a RangeError");
}
check(allocations === 0, "count overflow allocated");

const RealTextEncoder = globalThis.TextEncoder;
globalThis.TextEncoder = class { encode() { return { length: 0x1_0000_0000 }; } };
try {
  marshalAbi(new Map([["x", 1n]]), "map-string-int", bounded);
  throw new Error("key length overflow accepted");
} catch (error) {
  check(error instanceof RangeError, "key length overflow was not a RangeError");
}
check(allocations === 0, "key length overflow allocated");

globalThis.TextEncoder = class { encode() { return { length: 0xfffffff4 }; } };
try {
  marshalAbi(new Map([["x", 1n]]), "map-string-int", bounded);
  throw new Error("blob length overflow accepted");
} catch (error) {
  check(error instanceof RangeError, "blob length overflow was not a RangeError");
}
check(allocations === 0, "blob length overflow allocated");
globalThis.TextEncoder = RealTextEncoder;

try {
  marshalAbi(new Map([["x", 1n]]), "map-string-int", bounded);
  throw new Error("bounded write unexpectedly succeeded");
} catch (error) {
  check(error instanceof RangeError, "bounded write did not fail at the memory boundary");
}
check(frees.some(([ptr]) => ptr === 60), "post-allocation write failure leaked");

const highBit = {
  memory: { buffer: new ArrayBuffer(64) },
  jet_abi_map_string_int_alloc() { return -2147483648; },
  jet_abi_map_string_int_free(ptr, byteLen) { frees.push([ptr, byteLen]); },
};
try {
  marshalAbi(new Map([["x", 1n]]), "map-string-int", highBit);
  throw new Error("high-bit pointer unexpectedly fit bounded memory");
} catch (error) {
  check(error instanceof RangeError, "high-bit pointer did not reach the memory boundary");
}
check(frees.some(([ptr, len]) => ptr === 0x80000000 && len > 0), "signed allocation pointer was not freed as u32");

try {
  unmarshalAbi((0x80000000n << 32n) | 4n, "map-string-int", highBit);
  throw new Error("high-bit return pointer unexpectedly fit bounded memory");
} catch (error) {
  check(error instanceof RangeError, "high-bit return pointer did not reach the memory boundary");
}
check(frees.some(([ptr, len]) => ptr === 0x80000000 && len === 4), "high-bit return pointer was not freed as u32");
console.log("ok");
