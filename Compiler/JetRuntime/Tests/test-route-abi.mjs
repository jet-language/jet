#!/usr/bin/env node
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { loweringRuntimeAbi, parseRuntimeExports, runtimeExportSignatures, validateLoweringRuntimeAbi, validateRuntimeOwnership, validateRuntimeRoute } from "../../../Tools/agent/runtime-route-abi.mjs";

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
check(tuple, ["value: I64", "item0: I64", "item1: I64"], "-> ()");
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
const unit = parseRuntimeExports("#Export(c) fn unit(value: I64) -> () {}", "Source/Test.jet").get("unit");
validateRuntimeRoute(route([["i64"]], null), unit);
const nested = parseRuntimeExports("#Export(c) fn nested(value: (I64, I64)) {}", "Source/Test.jet").get("nested");
assert.throws(() => validateRuntimeRoute(route([["i64"], ["i64"]], null), nested), /does not match C ABI/);
validateRuntimeOwnership(new Set(parsed.keys()), parsed);
assert.throws(() => validateRuntimeOwnership(new Set(["jet_a"]), parsed), /jet_b is missing from exports.list/);
assert.throws(() => validateRuntimeOwnership(new Set([...parsed.keys(), "jet_missing"]), parsed), /jet_missing has no #Export\(c\) declaration/);
const lifecycle = parseRuntimeExports("// runtime-init: 00042\n#Export(c) fn init() {}\n// runtime-fini: 65535\n#Export(c) fn fini() {}", "Source/Test.jet");
assert.equal(lifecycle.get("init").lifecycle, "init 42");
assert.equal(lifecycle.get("fini").lifecycle, "fini 65535");
assert.throws(() => parseRuntimeExports("// runtime-init: 65536\n#Export(c) fn init() {}", "Source/Test.jet"), /lifecycle priority exceeds/);

// Lowering-only CString helpers have no predecessor Rust C route. Validate
// their real raw-word ABI without adding a second Core meaning.
const cStringDefinitions = new Map(loweringRuntimeAbi.map(({ name, params, ret }) => [
  name, definition(params.map((_, index) => `a${index}: I64`), ret ? "-> I64" : ""),
]));
validateLoweringRuntimeAbi(cStringDefinitions);
const missingCString = new Map(cStringDefinitions);
missingCString.delete("jet_rt_c_string_free");
assert.throws(() => validateLoweringRuntimeAbi(missingCString), /missing Jet lowering helper/);
for (const { name } of loweringRuntimeAbi) {
  const wrong = new Map(cStringDefinitions);
  wrong.set(name, definition(["pointer: F64"], "-> I64"));
  assert.throws(() => validateLoweringRuntimeAbi(wrong), /does not match C ABI/);
}
validateLoweringRuntimeAbi(new Map());

// Exercise the composed runtime's real ownership partition, not just synthetic
// signatures. Ported routes keep lowering metadata and have no Rust adapter.
const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const names = new Set(readFileSync(join(root, "Compiler/JetRuntime/exports.list"), "utf8").split("\n")
  .map((line) => line.trim()).filter((line) => line && !line.startsWith("#")).map((line) => line.split(/\s+/u)[0]));
validateRuntimeOwnership(names, runtimeExportSignatures(root));
validateLoweringRuntimeAbi(runtimeExportSignatures(root));
const routeText = readFileSync(join(root, "Compiler/JetBackend/Source/Lower/RuntimeRoutes.jet"), "utf8");
const routeNames = new Set([...routeText.matchAll(/LowerRoute\{symbol: "[^"]+", export: "([^"]+)"/g)].map((match) => match[1]));
for (const [, name, inline] of routeText.matchAll(/LowerRoute\{symbol: "[^"]+", export: "([^"]+)", native_lists: (true|false),/g)) {
  assert.equal(inline === "true", names.has(name), `${name} List representation must follow Jet ownership`);
}
assert.match(routeText, /pub native_lists: Bool/);
if (routeNames.has("jet_net_raw_resolve_ordered")) {
  assert.match(routeText, /LowerRecordSchema\{jet_name: "Jet__ResolveError", variants: "", fields: "kind=Int,has_os_code=Flag,os_code=Int,message=Text"\}/);
  assert.match(routeText, /jet_name: "<corelib>\/Core\/net::Core\/net\/net.jet::__ResolveError", runtime: "Jet__ResolveError"/);
  if (names.has("jet_net_raw_resolve_ordered")) {
    for (const name of ["jet_rt_record_new_Jet__ResolveError", "jet_rt_handle_clone_Jet__ResolveError", "jet_rt_handle_drop_Jet__ResolveError",
      ...["kind", "has_os_code", "os_code", "message"].map((field) => `jet_rt_record_get_Jet__ResolveError_${field}`)]) {
      assert.ok(names.has(name), `native resolver requires canonical record helper ${name}`);
    }
  }
}
assert.equal(routeNames.has("jet_rt_int_drop"), names.has("jet_rt_int_drop"), "Int destructor route must follow Jet ownership");
const rustText = readFileSync(join(root, "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs"), "utf8");
const rustNames = new Set([...rustText.matchAll(/pub extern "C" fn ([A-Za-z_]\w*)\(/g)].map((match) => match[1]));
const ported = [...names].filter((name) => routeNames.has(name));
assert.ok(ported.length > 0, "no real Jet-owned family exercised");
for (const name of names) assert.ok(!rustNames.has(name), `Jet-owned export ${name} still has a Rust adapter`);
const listOut = /    fn list_out\([\s\S]*?\n    \}/.exec(rustText)?.[0];
assert.ok(listOut?.includes("super::jet_c_abi::jet_rt_alloc(len * 8, 8)"), "List results must use the shared runtime heap");
assert.ok(!listOut.includes("std::alloc::alloc("), "List results must not escape Rust's heap");
for (const [name, stem] of [["IOError", "error"], ["IOContext", "context"], ["JetTaskFailure", "failure"]]) {
  if (!names.has(`jet_rt_handle_drop_${name}`)) continue;
  assert.ok(!new RegExp(`\\*(?:mut|const)\\s+crate::(?:jet_std::)?${name}\\b`).test(rustText), `${name} boundary still borrows Rust layout`);
  assert.ok(!new RegExp(`Box::new\\(crate::(?:jet_std::)?${name}\\b`).test(rustText), `${name} boundary still boxes a Rust value`);
  if (name === "IOError") {
    assert.ok(rustText.includes(`super::jet_c_abi_host::${stem}_handle(`));
    assert.ok(rustText.includes(`super::jet_c_abi_host::${stem}_view(`));
  }
}
const handText = readFileSync(join(root, "crates/jet-codegen/src/Prelude/Core/CAbi.rs"), "utf8");
assert.match(handText, /#\[cfg\(not\(jet_native_pack\)\)\]\s+mod host_records/);
const handRecords = handText.slice(handText.indexOf("mod host_records"), handText.indexOf("// TaskFailure remains"));
assert.ok(!/\bBox\b|crate::jet_std::(?:IOError|IOContext)|super::JetTaskFailure/.test(handRecords), "hand records must use C headers on the seed tier too");
assert.ok(!/to_owned|String::/.test(handRecords), "raw record projections must not create intermediate Rust Strings");
const hostText = readFileSync(join(root, "crates/jet-codegen/src/Prelude/Core/CAbiHost.rs"), "utf8");
for (const stem of ["error", "context", ...(names.has("jet_rt_handle_drop_JetTaskFailure") ? ["failure"] : [])]) {
  for (const operation of ["handle", "view", "take", "drop"]) assert.ok(hostText.includes(`fn ${stem}_${operation}(`), `missing ${stem}_${operation}`);
}
console.log("runtime route ABI: CHECK OK");
