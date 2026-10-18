#!/usr/bin/env node
import assert from "node:assert/strict";
import { parseRuntimeExports, validateRuntimeRoute } from "../../../Tools/agent/runtime-route-abi.mjs";

function definition(params, suffix = "") {
  return { params, suffix, path: "Source/Test.jet", lifecycle: "" };
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
check(plain, ["text: I64", "handle: U64", "word: I64", "flag: U8", "float: Float"], "-> Bool");
reject(plain, ["text: String", "handle: U64", "word: I64", "flag: U8", "float: Float"], "-> Bool");
reject(plain, ["text: I64", "handle: U64", "word: Int", "flag: U8", "float: Float"], "-> Bool");
reject(plain, ["text: I64", "handle: U64", "word: I64", "flag: I64", "float: Float"], "-> Bool");
reject(plain, ["text: I64", "handle: U64", "word: I64", "flag: U8", "float: I64"], "-> Bool");
reject(plain, ["text: I64", "handle: U64", "word: I64", "flag: U8", "float: Float"], "-> I64");

// List inputs flatten to data/length. List, Option, and Result outputs are
// addresses even when the pointed-to payload is a float or a narrow flag.
const composite = route([["*const u64", "i64"], ["JetCString"]], "i64", ["*mut u64", "i64", "f64", "bool"]);
const compositeParams = ["data: I64", "len: U64", "text: I64", "out_data: I64", "out_len: I64", "out_float: I64", "out_flag: I64"];
check(composite, compositeParams, "-> I64");
reject(composite, compositeParams.slice(0, -1), "-> I64");
reject(composite, [...compositeParams, "extra: I64"], "-> I64");
reject(composite, compositeParams.map((p) => p === "out_float: I64" ? "out_float: Float" : p), "-> I64");
reject(composite, compositeParams, "-> I64 Never!");
reject(composite, compositeParams.map((p) => p === "text: I64" ? "text: &I64" : p), "-> I64");
const tuple = route([["i64"]], null, ["i64", "JetCString"]);
check(tuple, ["value: I64", "item0: I64", "item1: I64"]);
check(tuple, ["value: I64", "item0: I64", "item1: I64"], "-> ()");
reject(tuple, ["value: I64", "item0: I64", "item1: I64"], "-> I64");
check(route([], null), []);
check(route([], "f64"), [], "-> Float");
reject(route([], "f64"), [], "-> I64");
// Only the reviewed resolver-constructor flag may widen to a masked word.
const resolverFlag = { ...route([["i64"], ["u8"], ["i64"], ["JetCString"]], "*mut crate::Jet__ResolveError"), name: "jet_rt_record_new_jet_resolve_error" };
check(resolverFlag, ["kind: I64", "flag: I64", "os_code: I64", "message: I64"], "-> I64");
reject({ ...resolverFlag, name: "jet_unlisted" }, ["kind: I64", "flag: I64", "os_code: I64", "message: I64"], "-> I64");
reject(resolverFlag, ["kind: I64", "flag: U64", "os_code: I64", "message: I64"], "-> I64");
reject({ ...resolverFlag, result: { ret: "u8", outs: [] } }, ["kind: I64", "flag: I64", "os_code: I64", "message: I64"], "-> I64");
const socketWords = { ...route([["*mut crate::JetIpAddr"], ["u16"], ["u32"], ["u32"]], "*mut crate::JetSocketAddr"), name: "jet_net_raw_socket_from_parts" };
check(socketWords, ["ip: I64", "port: I64", "flow: I64", "scope: I64"], "-> I64");
reject({ ...socketWords, name: "jet_unlisted" }, ["ip: I64", "port: I64", "flow: I64", "scope: I64"], "-> I64");
const resolverPort = { ...route([["JetCString"], ["u16"]], "i64", ["*mut u64", "i64", "JetCString"]), name: "jet_net_raw_resolve_ordered" };
check(resolverPort, ["host: I64", "port: I64", "data: I64", "len: I64", "error: I64"], "-> I64");
const maskedFlag = "#Export(c) fn jet_rt_record_new_jet_resolve_error(kind: I64, flag: I64, os: I64, text: I64) -> I64 { if (flag.bit_and(I64{255})) != I64{0} { return kind } kind }";
assert.ok(parseRuntimeExports(maskedFlag, "Source/Test.jet").has("jet_rt_record_new_jet_resolve_error"));
assert.throws(() => parseRuntimeExports(maskedFlag.replace("flag.bit_and(I64{255})", "flag"), "Source/Test.jet"), /low-8-bit mask/);
assert.throws(() => parseRuntimeExports(maskedFlag.replace("flag.bit_and(I64{255})", "flag /* flag.bit_and(I64{255}) */"), "Source/Test.jet"), /low-8-bit mask/);
assert.throws(() => parseRuntimeExports(maskedFlag.replace("flag.bit_and(I64{255})", "flag") + "\nfn unrelated(flag: I64) { flag.bit_and(I64{255}) }", "Source/Test.jet"), /low-8-bit mask/);
assert.throws(() => parseRuntimeExports(maskedFlag.replace("flag.bit_and(I64{255})", '"flag.bit_and(I64{255})"'), "Source/Test.jet"), /low-8-bit mask/);
assert.throws(() => parseRuntimeExports(maskedFlag.replace("flag.bit_and(I64{255})", "flag.bit_and(I64{65535})"), "Source/Test.jet"), /low-8-bit mask/);
const narrowFlag = maskedFlag.replace("flag: I64", "flag: U8").replace("(flag.bit_and(I64{255})) != I64{0}", "flag != U8{0}");
validateRuntimeRoute(resolverFlag, parseRuntimeExports(narrowFlag, "Source/Test.jet").get(resolverFlag.name));

// Canonical Float uses the f64 scalar ABI, never a word or an indirect out slot.
const floating = route([["f64"]], "f64");
check(floating, ["value: Float"], "-> Float");
reject(floating, [], "-> Float");
reject(floating, ["value: Float", "extra: I64"], "-> Float");
reject(floating, ["value: Float"], "-> Float Never!");
reject(floating, ["value: &Float"], "-> Float");
reject(floating, ["&value: Float"], "-> Float");
reject(route([["i64"]], "f64"), ["value: Float"], "-> Float");
for (const carrier of ["F64", "F32", "Float32"]) {
  reject(floating, [`value: ${carrier}`], "-> Float");
  reject(floating, ["value: Float"], `-> ${carrier}`);
}
reject(route([["f32"]], "f64"), ["value: Float"], "-> Float");
reject(route([["f64"]], "f32"), ["value: Float"], "-> Float");
const parsedFloat = parseRuntimeExports("#Export(c) fn jet_float(value: Float) -> Float { value }", "Source/Test.jet").get("jet_float");
assert.deepEqual(parsedFloat, definition(["value: Float"], "-> Float"));
validateRuntimeRoute(floating, parsedFloat);

const source = "#Export(c)\nfn jet_a(a: I64, b: U64,) -> I64 { return a }\n#Export(c) fn jet_b(out: I64) { }\nfn internal() { }\n";
const parsed = parseRuntimeExports(source, "Source/Test.jet");
assert.deepEqual([...parsed.keys()], ["jet_a", "jet_b"]);
assert.deepEqual(parsed.get("jet_a"), definition(["a: I64", "b: U64"], "-> I64"));
assert.deepEqual(parsed.get("jet_b"), definition(["out: I64"]));
assert.throws(() => parseRuntimeExports(source + "#Export(c) fn jet_a() {}", "Source/Test.jet"), /duplicate/);
assert.throws(() => parseRuntimeExports("#Export(c) fn generic<T>(value: T) {}", "Source/Test.jet"), /unparsed/);
const failing = parseRuntimeExports("#Export(c) fn failing(value: I64) -> I64 Never! {}", "Source/Test.jet").get("failing");
assert.throws(() => validateRuntimeRoute(route([["i64"]], "i64"), failing), /does not match C ABI/);
const lifecycle = parseRuntimeExports("// runtime-init: 3\n#Export(c) fn jet_init() { }\n// runtime-fini: 1\n#Export(c) fn jet_fini() { }\n", "Source/Test.jet");
assert.equal(lifecycle.get("jet_init").lifecycle, "init 3");
assert.equal(lifecycle.get("jet_fini").lifecycle, "fini 1");
assert.throws(() => parseRuntimeExports("// runtime-init: 65536\n#Export(c) fn jet_init() { }\n", "Source/Test.jet"), /exceeds 65535/);
// Record Lists cross every route as handle buffers (Lower/Routes.jet
// lower_record_list_element), whatever the provider's ownership. The census
// below freezes the routes whose carriers hold a List of a schema record, so
// a new Jet provider returning or reading one is reviewed against that
// contract (a provider handing back inline rows surfaces as a pointer-sized
// allocation in the first `entry.field` read), and keeps the native witness
// that reads a field of `files.list_dir`'s result.
{
  const { readFileSync } = await import("node:fs");
  const { dirname, join, resolve } = await import("node:path");
  const { fileURLToPath } = await import("node:url");
  const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
  const routeText = readFileSync(join(root, "Compiler/JetBackend/Source/Lower/RuntimeRoutes.jet"), "utf8");
  const schemas = new Set([...routeText.matchAll(/LowerRecordSchema\{jet_name: "([^"]+)"/g)].map((match) => match[1]));
  const recordLists = new Set();
  for (const [, symbol, carriers] of routeText.matchAll(/LowerRoute\{symbol: "([^"]+)", export: "[^"]+", native_lists: (?:true|false), (params: .*?, result: LowerRouteResult\..*?)\},\n/g)) {
    for (const [, name] of carriers.matchAll(/List\{elem: LowerRouteCarrier\.Handle\{name: "([^"]+)"\}\}/g)) if (schemas.has(name)) recordLists.add(symbol);
  }
  // Seed (Rust) adapters always exchanged handle buffers; the Jet-owned rows
  // (jet_dyn_field, jet_rt_codec_*, jet_std_fs_list_dir_raw, message_parts)
  // build or read the same shape.
  const reviewed = [
    "jet_binary_pattern_match", "jet_dyn_at", "jet_dyn_bool", "jet_dyn_field", "jet_dyn_float", "jet_dyn_int", "jet_dyn_text",
    "jet_email::envelope", "jet_email::envelope_parts", "jet_email::message_parts", "jet_log_record_emit_raw",
    "jet_net_dns_srv", "jet_net_dns_srv_at_lookup", "jet_net_dns_srv_lookup",
    "jet_ring_log_debug_fields", "jet_ring_log_error_fields", "jet_ring_log_info_fields", "jet_ring_log_warn_fields",
    "jet_rt_codec_decode_bool", "jet_rt_codec_decode_char", "jet_rt_codec_decode_float", "jet_rt_codec_decode_i16", "jet_rt_codec_decode_i32",
    "jet_rt_codec_decode_i64", "jet_rt_codec_decode_i8", "jet_rt_codec_decode_int", "jet_rt_codec_decode_string", "jet_rt_codec_decode_u16",
    "jet_rt_codec_decode_u32", "jet_rt_codec_decode_u64", "jet_rt_codec_decode_u8", "jet_rt_codec_parse_json", "jet_rt_codec_parse_toml",
    "jet_rt_codec_parse_yaml", "jet_rt_codec_under", "jet_std_fs_list_dir_raw", "jet_text_pattern_match",
  ].filter((symbol) => routeText.includes(`LowerRoute{symbol: "${symbol}"`));
  assert.deepEqual([...recordLists].sort(), reviewed.sort(), "a route carrying a List of schema records must be reviewed against the handle-buffer contract");
  const fixture = readFileSync(join(root, "Compiler/JetBackend/Tests/Fixtures/Values.jet"), "utf8");
  assert.match(fixture, /expect-requires-route qualified-dir-entries jet_std_fs_list_dir_raw/, "the native list_dir witness must read a DirEntry field");
}
console.log("runtime route ABI: CHECK OK");
