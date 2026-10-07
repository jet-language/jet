#!/usr/bin/env node
import assert from "node:assert/strict";
import { parseRuntimeExports, validateRuntimeRoute } from "../../../Tools/agent/runtime-route-abi.mjs";

function definition(params, suffix = "") {
  return { params, suffix, path: "Source/Test.jet" };
}
function route(params, ret, outs = []) {
  return { symbol: "jet_test_route", name: "jet_test_route", params: params.map((ctys) => ({ ctys })), result: { ret, outs: outs.map((cty) => ({ cty })) } };
}
function check(adapter, params, suffix = "") {
  validateRuntimeRoute(adapter, definition(params, suffix));
}
function reject(adapter, params, suffix = "") {
  assert.throws(() => check(adapter, params, suffix), /does not match C ABI/);
}

// Text and handle pointers, exact-Int carrier words, byte flags, float registers.
const plain = route([["JetCString"], ["*mut crate::Value"], ["i64"], ["u8"], ["f64"]], "bool");
check(plain, ["text: I64", "handle: U64", "word: I64", "flag: U8", "float: F64"], "-> Bool");
reject(plain, ["text: String", "handle: U64", "word: I64", "flag: U8", "float: F64"], "-> Bool");
reject(plain, ["text: I64", "handle: U64", "word: Int", "flag: U8", "float: F64"], "-> Bool");
reject(plain, ["text: I64", "handle: U64", "word: I64", "flag: I64", "float: F64"], "-> Bool");
reject(plain, ["text: I64", "handle: U64", "word: I64", "flag: U8", "float: I64"], "-> Bool");
reject(plain, ["text: I64", "handle: U64", "word: I64", "flag: U8", "float: F64"], "-> I64");

// List inputs flatten to data/length. List, Option, and Result outputs are
// addresses even when the pointed-to payload is a float or a narrow flag.
const composite = route([["*const u64", "i64"], ["JetCString"]], "i64", ["*mut u64", "i64", "f64", "bool"]);
const compositeParams = ["data: I64", "len: U64", "text: I64", "out_data: I64", "out_len: I64", "out_float: I64", "out_flag: I64"];
check(composite, compositeParams, "-> I64");
reject(composite, compositeParams.slice(0, -1), "-> I64");
reject(composite, [...compositeParams, "extra: I64"], "-> I64");
reject(composite, compositeParams.map((p) => p === "out_float: I64" ? "out_float: F64" : p), "-> I64");
reject(composite, compositeParams, "-> I64 Never!");
reject(composite, compositeParams.map((p) => p === "text: I64" ? "text: &I64" : p), "-> I64");
const tuple = route([["i64"]], null, ["i64", "JetCString"]);
check(tuple, ["value: I64", "item0: I64", "item1: I64"]);
reject(tuple, ["value: I64", "item0: I64", "item1: I64"], "-> I64");
check(route([], null), []);
check(route([], "f64"), [], "-> F64");
reject(route([], "f64"), [], "-> I64");

const source = "#Export(c)\nfn jet_a(a: I64, b: U64,) -> I64 { return a }\n#Export(c) fn jet_b(out: I64) { }\nfn internal() { }\n";
const parsed = parseRuntimeExports(source, "Source/Test.jet");
assert.deepEqual([...parsed.keys()], ["jet_a", "jet_b"]);
assert.deepEqual(parsed.get("jet_a"), definition(["a: I64", "b: U64"], "-> I64"));
assert.deepEqual(parsed.get("jet_b"), definition(["out: I64"]));
assert.throws(() => parseRuntimeExports(source + "#Export(c) fn jet_a() {}", "Source/Test.jet"), /duplicate/);
assert.throws(() => parseRuntimeExports("#Export(c) fn generic<T>(value: T) {}", "Source/Test.jet"), /unparsed/);
const failing = parseRuntimeExports("#Export(c) fn failing(value: I64) -> I64 Never! {}", "Source/Test.jet").get("failing");
assert.throws(() => validateRuntimeRoute(route([["i64"]], "i64"), failing), /does not match C ABI/);
console.log("runtime route ABI: CHECK OK");
