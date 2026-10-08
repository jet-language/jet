#!/usr/bin/env node
import assert from "node:assert/strict";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { generatedRecordPath, recordApplication, recordApplicationsIn, recordBindings, recordInstanceName, substituteRecordType } from "../../../Tools/agent/runtime-route-abi.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const scratch = process.env.JET_TEST_SCRATCH ?? join(process.env.HOME, ".cache/jet-dev/scratch");
mkdirSync(scratch, { recursive: true });
const fixture = mkdtempSync(join(scratch, "generic-route-schema-"));
function put(path, body) {
  const target = join(fixture, path);
  mkdirSync(dirname(target), { recursive: true });
  writeFileSync(target, body);
}
function run(check = false) {
  return spawnSync(process.execPath, [join(fixture, "Tools/agent/gen-c-abi-routes.mjs"), join(fixture, "runtime.rs"), ...(check ? ["--check"] : [])], { encoding: "utf8" });
}
function success(result) { assert.equal(result.status, 0, result.stderr + result.stdout); }
const measurement = recordInstanceName("JetMeasurement", ["f64"]);
const pair = recordInstanceName("JetPair", ["f64", "bool"]);
assert.notEqual(measurement, recordInstanceName("JetMeasurement", ["crate::jet_foundation::Numeric::JetInt"]));
assert.notEqual(recordInstanceName("Pair", ["A", "BC"]), recordInstanceName("Pair", ["AB", "C"]));
assert.equal(recordInstanceName("Plain", []), "Plain");
const bindings = recordBindings("struct Pair<A: Clone, B>", ["f64", "bool"], "Pair<f64,bool>");
assert.equal(substituteRecordType("pub first: A", bindings), "pub first: f64");
assert.equal(substituteRecordType("pub second: B", bindings), "pub second: bool");
assert.equal(substituteRecordType("pub stable: AA", bindings), "pub stable: AA");
assert.throws(() => recordBindings("struct Pair<A, B>", ["f64"], "Pair<f64>"), /Pair<f64>.*expected 2/);
assert.throws(() => recordApplication("Pair<Vec<f64>,bool>"), /Pair<Vec<f64>,bool>.*nested/);
assert.equal(generatedRecordPath("grades.curve"), "__jet_grades_dcurve");
assert.equal(generatedRecordPath("a_b::c/d"), "__jet_a_ub_c_cc_sd");
assert.deepEqual(recordApplicationsIn("fn value(x: Pair<f64,bool>) -> Option<Pair<f64,bool>>", "Pair"), ["Pair<f64,bool>", "Pair<f64,bool>"]);

try {
  for (const name of ["gen-c-abi-routes.mjs", "runtime-route-abi.mjs", "native-route-handles.mjs"]) {
    const path = `Tools/agent/${name}`;
    mkdirSync(dirname(join(fixture, path)), { recursive: true });
    cpSync(join(root, path), join(fixture, path));
  }
  put("crates/jet-codegen/src/Codegen/MIRRust.rs", `
fn core_call() { if row.module == "core.fixture" && row.member == "indirect" {} }
fn source_math_call() { if row.module != "core.math" && !matches!(row.member.as_str(), "abs") {} }
fn exact_prelude_adapter() { match symbol { "jet_exact" => ("jet_exact_kernel", &[]), } }
fn rust_apply_type() { if name.name == "Fixture" { return self.rust_root_prelude_type("JetFixture", args); } }
fn core_native_probe_name() {}
fn is_source_owned_core_compute_shape() {}
fn is_source_owned_core_net_shape() {}
fn is_source_owned_core_web_query_shape() {}
`);
  const table = `match name { "Fixture" => Some("JetFixture"), "Measurement" => Some("JetMeasurement"), "Pair" => Some("JetPair"), }`;
  put("crates/jet-codegen/src/Codegen/Context.rs", ["root_prelude_rust_type_name", "job_queue_rust_type", "net_handle_rust_type", "core_rust_type_name", "file_handle_rust_type", "core_email_rust_type_name", "compute_handle_rust_type", "service_handle_rust_type", "layout_handle_rust_type"].map((name) => `fn ${name}() { ${table} }`).join("\n"));
  put("crates/jet-foundation/src/Syntax.rs", "");
  put("crates/jet-foundation/src/Syntax/predicates.rs", ["reflect_handle_rust_type", "binary_text_handle_rust_type", "alloc_handle_rust_type"].map((name) => `fn ${name}() { ${table} }`).join("\n"));
  put("crates/jet-codegen/src/Codegen/TIR/routes.rs", "");
  put("crates/jet-codegen/src/Prelude/Core/CAbi.rs", "");
  put("Compiler/JetRuntime/sources.list", "Compiler/JetRuntime/Source/Fixture.jet\n");
  put("Compiler/JetRuntime/Source/Fixture.jet", "#Export(c) fn jet_fixture_scalar(value: F64) -> F64 { value }\n");
  put("Compiler/JetRuntime/exports.list", "jet_fixture_scalar\n");
  put("Compiler/JetBackend/Source/Lower/RuntimeRoutes.jet", "");
  put("Core/units/units.jet", "pub struct Measurement<T> { value: T, uncertainty: T }\npub struct Pair<A,B> { first: A, second: B }\n");
  const registry = (extra) => `CoreCallRecord::new("core.fixture", "scalar", "jet_fixture_scalar", true)\nCoreCallRecord::new("core.fixture", "plain", "jet_fixture_plain", true)\n${extra}`;
  const declarations = `
#[derive(Clone, PartialEq)]
pub struct JetFixture { pub value: f64 }
#[derive(Clone, PartialEq)]
pub struct JetMeasurement<T: Copy> { value: T, uncertainty: T }
#[derive(Clone, PartialEq)]
pub struct JetPair<A, B> { first: A, second: B }
pub fn jet_fixture_scalar(value: f64) -> f64 { value }
pub fn jet_fixture_plain(value: JetFixture) -> JetFixture { value }
`;
  put("crates/jet-foundation/src/Syntax/core_calls.rs", registry(""));
  put("runtime.rs", declarations);
  success(run());
  success(run(true));
  const baselineRust = readFileSync(join(fixture, "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs"), "utf8");
  const baselineJet = readFileSync(join(fixture, "Compiler/JetBackend/Source/Lower/RuntimeRoutes.jet"), "utf8");
  const genericRows = `CoreCallRecord::new("core.fixture", "measurement", "jet_fixture_measurement", true)\nCoreCallRecord::new("core.fixture", "pair", "jet_fixture_pair", true)\n`;
  put("crates/jet-foundation/src/Syntax/core_calls.rs", registry(genericRows));
  put("runtime.rs", declarations + `pub fn jet_fixture_measurement(value: JetMeasurement<f64>) -> JetMeasurement<f64> { value }\npub fn jet_fixture_pair(value: JetPair<f64,bool>) -> JetPair<f64,bool> { value }\n`);
  success(run());
  success(run(true));
  const rust = readFileSync(join(fixture, "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs"), "utf8");
  const jet = readFileSync(join(fixture, "Compiler/JetBackend/Source/Lower/RuntimeRoutes.jet"), "utf8");
  assert.ok(jet.includes(`LowerRecordSchema{jet_name: "${measurement}", variants: "", fields: "value=Float,uncertainty=Float"}`));
  assert.ok(jet.includes(`LowerRecordSchema{jet_name: "${pair}", variants: "", fields: "first=Float,second=Flag"}`));
  assert.ok(jet.includes(`jet_name: "<corelib>/Core/units::Core/units/units.jet::Measurement<Float>", runtime: "${measurement}"`));
  assert.ok(jet.includes(`jet_name: "<corelib>/Core/units::Core/units/units.jet::Pair<Float,Bool>", runtime: "${pair}"`));
  assert.ok(rust.includes(`jet_rt_record_new_${measurement}(m0: f64, m1: f64)`));
  assert.ok(rust.includes("crate::JetMeasurement::<f64> { value: m0, uncertainty: m1 }"));
  // Existing schema, handle and helper definitions remain byte-identical.
  for (const row of baselineJet.split("\n").filter((line) => /Lower(?:RuntimeHandle|RecordSchema)\{/.test(line))) assert.ok(jet.includes(row));
  const helper = (source) => [...source.matchAll(/    #\[no_mangle\]\n    pub extern "C" fn jet_rt_(?:handle|record)_\w*JetFixture\w*\([^]*?\n    \}/g)].map((match) => match[0]);
  assert.deepEqual(helper(rust), helper(baselineRust));
  assert.ok(helper(baselineRust).length > 0, "non-generic helper comparison must exercise real definitions");
  // Source-owned generics have actual mangled declarations and private,
  // mangled fields, not a historical native carrier name or a C route.
  const sampleKey = "<corelib>/Core/fixture::Core/fixture/fixture.jet::Sample";
  const pairKey = "<corelib>/Core/fixture::Core/fixture/fixture.jet::Pair";
  const sampleSource = generatedRecordPath(sampleKey), pairSource = generatedRecordPath(pairKey);
  put("Core/fixture/fixture.jet", "pub struct Sample<T> { value: T, uncertainty: T }\npub struct Pair<A,B> { first: A, second: B }\n");
  put("runtime.rs", declarations + `
#[derive(Clone)]
pub struct ${sampleSource}<T> { __jet_value: T, __jet_uncertainty: T }
#[derive(Clone)]
pub struct ${pairSource}<A,B> { __jet_first: A, __jet_second: B }
pub fn source_sample(value: f64) -> ${sampleSource}<f64> { ${sampleSource} { __jet_value: value, __jet_uncertainty: value } }
pub fn source_pair(first: f64, second: bool) -> ${pairSource}<f64,bool> { ${pairSource} { __jet_first: first, __jet_second: second } }
`);
  success(run());
  success(run(true));
  const sourceJet = readFileSync(join(fixture, "Compiler/JetBackend/Source/Lower/RuntimeRoutes.jet"), "utf8");
  const sourceRust = readFileSync(join(fixture, "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs"), "utf8");
  const sampleInstance = recordInstanceName(sampleSource, ["f64"]), pairInstance = recordInstanceName(pairSource, ["f64", "bool"]);
  assert.ok(sourceJet.includes(`LowerRecordSchema{jet_name: "${sampleInstance}", variants: "", fields: "value=Float,uncertainty=Float"}`));
  assert.ok(sourceJet.includes(`LowerRecordSchema{jet_name: "${pairInstance}", variants: "", fields: "first=Float,second=Flag"}`));
  assert.ok(sourceJet.includes(`jet_name: "${sampleKey}<Float>", runtime: "${sampleInstance}"`));
  assert.ok(sourceJet.includes(`jet_name: "${pairKey}<Float,Bool>", runtime: "${pairInstance}"`));
  assert.ok(sourceJet.includes(`LowerNativeType{jet_name: "${sampleKey}<Float>", declaration: "${sampleKey}", arguments: "Float"}`));
  assert.ok(sourceJet.includes(`LowerNativeExport{symbol: "jet_rt_handle_clone_${sampleInstance}", jet_name: "${sampleKey}<Float>", op: "handle.clone", member: ""}`));
  assert.ok(sourceRust.includes(`${sampleSource}::<f64> { __jet_value: m0, __jet_uncertainty: m1 }`));
  put("runtime.rs", declarations + "pub fn jet_fixture_measurement(value: JetMeasurement<Unbound>) -> JetMeasurement<f64> { value }\n");
  const unresolved = run();
  assert.notEqual(unresolved.status, 0);
  assert.match(unresolved.stderr, /JetMeasurement<Unbound>.*unresolved or unsupported argument Unbound/);
  for (const argument of ["u8", "i64", "u64", "f32"]) {
    put("runtime.rs", declarations + `pub fn jet_fixture_measurement(value: JetMeasurement<${argument}>) -> JetMeasurement<f64> { value }\n`);
    const unsupported = run();
    assert.notEqual(unsupported.status, 0, `${argument} must not create an unmatched Lower row`);
    assert.ok(unsupported.stderr.includes(`JetMeasurement<${argument}>`) && unsupported.stderr.includes(`unsupported argument ${argument}`), unsupported.stderr);
  }
  put("runtime.rs", declarations + "pub fn jet_fixture_measurement(value: JetMeasurement<Vec<f64>>) -> JetMeasurement<f64> { value }\n");
  const nested = run();
  assert.notEqual(nested.status, 0);
  assert.match(nested.stderr, /JetMeasurement<Vec<f64>>.*nested/);

  // Stream fixtures use the real Rust verification kernels. First inspect
  // retained seed marshalling, then compose one complete Jet-owned family.
  const stream = readFileSync(join(root, "crates/jet-foundation/src/StreamCursor.rs"), "utf8");
  const streamNames = [...stream.matchAll(/pub fn (jet_(?:reader|cursor)_\w+)\(/g)].map((match) => match[1]).filter((name) =>
    /^jet_cursor_(over|skip_ws|take_until)$/.test(name) || /^jet_reader_(over|over_owned|peek|seek|skip|take|remaining|at_end|read_[uif](8|16|32|64)(_le|_be)?)$/.test(name));
  assert.equal(streamNames.length, 29);
  const streamRows = streamNames.map((name) => `CoreCallRecord::new("core.fixture", "${name}", "${name}", true).with_native_int_result()`).join("\n");
  put("crates/jet-foundation/src/Syntax/core_calls.rs", registry(streamRows));
  put("runtime.rs", declarations + stream);
  success(run());
  const seed = readFileSync(join(fixture, "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs"), "utf8");
  const body = (name) => new RegExp(`pub extern "C" fn ${name}\\([^]*?\\n    \\}`).exec(seed)?.[0];
  for (const name of streamNames.filter((name) => /read_[ui]|peek/.test(name))) {
    assert.ok(body(name));
    assert.ok(!body(name).includes("jet_int_from_i64"), `${name} must preserve raw fixed bits`);
  }
  for (const endian of ["le", "be"]) assert.ok(body(`jet_reader_read_f32_${endian}`).includes("to_bits() as i64"));
  assert.ok(body("jet_reader_remaining").includes("jet_int_from_i64"));
  const streamDefinitions = streamNames.map((name) => {
    if (name === "jet_cursor_over") return [name, "text: I64", "I64"];
    if (name === "jet_cursor_skip_ws") return [name, "handle: I64", ""];
    if (name === "jet_cursor_take_until") return [name, "handle: I64, delimiter: I64, ok: I64, error: I64", "I64"];
    if (name === "jet_reader_over" || name === "jet_reader_over_owned") return [name, "data: I64, len: I64, stride: I64", "I64"];
    if (name === "jet_reader_take") return [name, "handle: I64, n: I64, stride: I64, ok: I64, ok_len: I64, error: I64", "I64"];
    if (name === "jet_reader_seek" || name === "jet_reader_skip") return [name, "handle: I64, n: I64, error: I64", "I64"];
    if (name === "jet_reader_remaining") return [name, "handle: I64", "I64"];
    if (name === "jet_reader_at_end") return [name, "handle: I64", "Bool"];
    return [name, "handle: I64, ok: I64, error: I64", "I64"];
  });
  for (const type of ["JetReader", "JetCursor"]) {
    streamDefinitions.push([`jet_rt_handle_clone_${type}`, "handle: I64", "I64"], [`jet_rt_handle_drop_${type}`, "handle: I64", ""]);
  }
  streamDefinitions.push(["jet_rt_handle_field_JetCursor_buf", "handle: I64, output: I64", ""]);
  put("Compiler/JetRuntime/Source/Fixture.jet", "#Export(c) fn jet_fixture_scalar(value: F64) -> F64 { value }\n" +
    streamDefinitions.map(([name, params, ret]) => `#Export(c) fn ${name}(${params})${ret ? ` -> ${ret}` : ""} {}`).join("\n"));
  put("Compiler/JetRuntime/exports.list", ["jet_fixture_scalar", ...streamDefinitions.map(([name]) => name)].join("\n") + "\n");
  success(run());
  success(run(true));
  const owned = readFileSync(join(fixture, "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs"), "utf8");
  for (const [name] of streamDefinitions) assert.ok(!owned.includes(`pub extern "C" fn ${name}(`), `${name} retains a Rust C body`);
  const streamMetadata = readFileSync(join(fixture, "Compiler/JetBackend/Source/Lower/RuntimeRoutes.jet"), "utf8");
  assert.match(streamMetadata, /export: "jet_reader_over", native_lists: true, params: \[LowerRouteCarrier\]\{LowerRouteCarrier.List\{elem: LowerRouteCarrier.Word\}\}/);
  assert.match(streamMetadata, /export: "jet_reader_take", native_lists: true, params: \[LowerRouteCarrier\]\{LowerRouteCarrier.HandleMut\{name: "JetReader"\}, LowerRouteCarrier.Word\}/);
  put("Compiler/JetRuntime/exports.list", ["jet_fixture_scalar", ...streamDefinitions.map(([name]) => name).filter((name) => name !== "jet_rt_handle_clone_JetReader")].join("\n") + "\n");
  const partial = run();
  assert.notEqual(partial.status, 0);
  assert.match(partial.stderr, /StreamCursor32 cutover.*missing jet_rt_handle_clone_JetReader/);
  console.log("CHECK OK — generic schemas, typed stream ABI and generator --check fixtures");
} finally {
  rmSync(fixture, { recursive: true, force: true });
}
