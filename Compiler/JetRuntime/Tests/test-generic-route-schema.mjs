#!/usr/bin/env node
import { runtimeAbiAtom } from "../../../Tools/agent/runtime-abi-symbols.mjs";
import assert from "node:assert/strict";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { generatedRecordPath, nativeProviderHandles, parseRuntimeExports, recordApplication, recordApplicationsIn, recordBindings, recordInstanceName, substituteRecordType } from "../../../Tools/agent/runtime-route-abi.mjs";

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
assert.equal(recordApplication("Option<&String>"), null);
assert.equal(recordApplication("core::option::Option<&String>"), null);
assert.equal(recordApplication("Result<Vec<f64>, String>"), null);
assert.deepEqual(recordApplication("crate::Option<f64>"), { base: "crate::Option", args: ["f64"] });
assert.equal(generatedRecordPath("grades.curve"), "__jet_grades_dcurve");
assert.equal(generatedRecordPath("a_b::c/d"), "__jet_a_ub_c_cc_sd");
assert.deepEqual(recordApplicationsIn("fn value(x: Pair<f64,bool>) -> Option<Pair<f64,bool>>", "Pair"), ["Pair<f64,bool>", "Pair<f64,bool>"]);
assert.deepEqual(recordApplicationsIn("fn x(v: crate::module::Pair<f64,bool>) -> super::Pair<f64,bool>", "Pair"),
  ["crate::module::Pair<f64,bool>", "super::Pair<f64,bool>"]);

const pipeDefinitions = ["ProcessStdoutStream", "ProcessStderrStream"].flatMap((type) => [
  `#Export(c) fn jet_rt_handle_clone_${runtimeAbiAtom(type)}(value: I64) -> I64 { value }`,
  `#Export(c) fn jet_rt_handle_drop_${runtimeAbiAtom(type)}(value: I64) {}`,
]).concat([
  "#Export(c) fn jet_process_pipe_take_raw(fd: I64) -> I64 { fd }",
  "#Export(c) fn jet_process_stream_line_bytes_raw(pipe: I64, data: I64, len: I64, error: I64) -> I64 { I64{1} }",
]).join("\n");
const pipeAbi = parseRuntimeExports(pipeDefinitions, "PipeFixture.jet");
assert.deepEqual(nativeProviderHandles(new Map()), []);
assert.equal(nativeProviderHandles(pipeAbi).length, 4);
const incompletePipe = new Map(pipeAbi);
incompletePipe.delete("jet_rt_handle_drop_process_stderr_stream");
assert.throws(() => nativeProviderHandles(incompletePipe), /native Pipe24 provider requires jet_rt_handle_drop_process_stderr_stream/);
const wrongPipe = parseRuntimeExports(pipeDefinitions.replace("len: I64", "len: Float"), "PipeFixture.jet");
assert.throws(() => nativeProviderHandles(wrongPipe), /line_bytes_raw.*does not match C ABI/);

try {
  for (const name of ["gen-c-abi-routes.mjs", "runtime-route-abi.mjs", "runtime-abi-symbols.mjs", "native-route-handles.mjs", "native-route-records.mjs", "native-route-environment.mjs", "native-route-partition.mjs"]) {
    const path = `Tools/agent/${name}`;
    mkdirSync(dirname(join(fixture, path)), { recursive: true });
    cpSync(join(root, path), join(fixture, path));
  }
  put("crates/jet-foundation/src/CoreModuleExports.rs", readFileSync(join(root, "crates/jet-foundation/src/CoreModuleExports.rs"), "utf8"));
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
  const table = `match name { "Fixture" => Some("JetFixture"), "Measurement" => Some("JetMeasurement"), "Pair" => Some("JetPair"), "Scalar" => Some("JetScalar"), }`;
  put("crates/jet-codegen/src/Codegen/Context.rs", ["root_prelude_rust_type_name", "job_queue_rust_type", "net_handle_rust_type", "core_rust_type_name", "file_handle_rust_type", "core_email_rust_type_name", "compute_handle_rust_type", "service_handle_rust_type", "layout_handle_rust_type"].map((name) => `fn ${name}() { ${table} }`).join("\n"));
  put("crates/jet-foundation/src/Syntax.rs", "");
  put("crates/jet-foundation/src/Syntax/predicates.rs", ["reflect_handle_rust_type", "binary_text_handle_rust_type", "alloc_handle_rust_type"].map((name) => `fn ${name}() { ${table} }`).join("\n"));
  put("crates/jet-codegen/src/Codegen/TIR/routes.rs", "");
  put("crates/jet-codegen/src/Prelude/Core/CAbi.rs", "");
  put("Compiler/JetRuntime/sources.list", "Compiler/JetRuntime/Source/Fixture.jet\n");
  put("Compiler/JetRuntime/Source/Fixture.jet", "#Export(c) fn jet_fixture_scalar(value: Float) -> Float { value }\n");
  put("Compiler/JetRuntime/exports.list", "jet_fixture_scalar\n");
  put("Compiler/JetBackend/Source/Lower/RuntimeRoutes.jet", "");
  put("Core/units/units.jet", "struct Measurement<T> { value: T, uncertainty: T }\nstruct Pair<A,B> { first: A, second: B }\n");
  const registry = (extra) => `CoreCallRecord::new("core.fixture", "scalar", "jet_fixture_scalar", true)\nCoreCallRecord::new("core.fixture", "plain", "jet_fixture_plain", true)\n${extra}`;
  // Canonical scalar codec adapters also require the real FieldError Clone
  // declaration (Prelude/CoreLib/JetStd/Dyn.rs:11-15), even in this
  // metadata-only fixture. Do not weaken that production ownership gate.
  const fieldErrorSource = readFileSync(join(root, "crates/jet-codegen/src/Prelude/CoreLib/JetStd/Dyn.rs"), "utf8");
  const fieldErrorDeclaration = /#\[derive\(Clone, Debug, PartialEq\)\]\npub struct FieldError \{[^]*?\n\}/.exec(fieldErrorSource)?.[0];
  assert.ok(fieldErrorDeclaration, "canonical FieldError declaration must be present");
  const declarations = `
#[derive(Clone, PartialEq)]
pub struct JetFixture { pub value: f64 }
#[derive(Clone, PartialEq)]
pub struct JetMeasurement<T: Copy> { value: T, uncertainty: T }
#[derive(Clone, PartialEq)]
pub struct JetPair<A, B> { first: A, second: B }
pub fn jet_fixture_scalar(value: f64) -> f64 { value }
pub fn jet_fixture_plain(value: JetFixture) -> JetFixture { value }
pub mod jet_std {
${fieldErrorDeclaration}
}
`;
  // Preserve the canonical borrowed optional-text parameter codec. This is
  // not admission of a nullable owned-Text result or a new generic record.
  put("crates/jet-foundation/src/Syntax/core_calls.rs", registry('CoreCallRecord::new("core.fixture", "optional", "jet_fixture_optional", true)\n'));
  put("runtime.rs", declarations + "pub fn jet_fixture_optional(value: Option<&String>) -> f64 { 0.0 }\n");
  success(run());
  assert.match(readFileSync(join(fixture, "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs"), "utf8"), /fn jet_fixture_optional\(a0: JetCString\) -> f64/);
  const borrowed = declarations + "#[derive(Clone, PartialEq)]\npub struct JetBinMatchPart<'a> { pub bytes: &'a [u8] }\npub fn jet_fixture_parts(value: Vec<JetBinMatchPart<'_>>) -> f64 { 0.0 }\n";
  put("crates/jet-foundation/src/Syntax/core_calls.rs", registry('CoreCallRecord::new("core.fixture", "parts", "jet_fixture_parts", true)\n'));
  put("runtime.rs", borrowed);
  success(run());
  const borrowedRust = readFileSync(join(fixture, "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs"), "utf8");
  assert.ok(borrowedRust.includes("jet_rt_handle_drop_jet_bin_match_part"));
  assert.ok(borrowedRust.includes("crate::JetBinMatchPart<'_>"));
  assert.ok(!borrowedRust.includes("jet_rt_record_new_jet_bin_match_part"));
  const borrowedList = spawnSync(process.execPath, [join(fixture, "Tools/agent/gen-c-abi-routes.mjs"), join(fixture, "runtime.rs"), "--native-list-only"], { encoding: "utf8" });
  success(borrowedList);
  const borrowedInventory = JSON.parse(borrowedList.stdout);
  assert.ok(borrowedInventory.provenance.borrowedCarriers.some((row) => row.runtime === "JetBinMatchPart" && row.qualified.includes("<'_>")));
  assert.ok(!borrowedInventory.nativeTypes.some((row) => row.runtime === "JetBinMatchPart"));
  put("runtime.rs", declarations + "#[derive(Clone)]\npub struct JetBorrowed<'a, T> { pub bytes: &'a [u8], pub value: T }\npub fn jet_fixture_parts(value: Vec<JetBorrowed<'_, f64>>) -> f64 { 0.0 }\n");
  const mixed = run();
  assert.notEqual(mixed.status, 0);
  assert.match(mixed.stderr, /mixed lifetime and TYPE arguments are unsupported/);
  const sumAlias = "pub type JetOutcome<T, E> = Result<T, E>;\npub struct JetAbsent;\n";
  put("crates/jet-foundation/src/Syntax/core_calls.rs", registry('CoreCallRecord::new("core.fixture", "tuple", "jet_fixture_tuple", true)\n'));
  put("runtime.rs", declarations + sumAlias + "pub fn jet_fixture_tuple() -> (JetOutcome<jet_foundation::Numeric::JetInt,JetAbsent>, f64) { unreachable!() }\n");
  success(run());
  assert.ok(readFileSync(join(fixture, "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs"), "utf8").includes("result type (JetOutcome<jet_foundation::Numeric::JetInt,JetAbsent>, f64)"));
  const sumList = spawnSync(process.execPath, [join(fixture, "Tools/agent/gen-c-abi-routes.mjs"), join(fixture, "runtime.rs"), "--native-list-only"], { encoding: "utf8" });
  success(sumList);
  assert.match(JSON.parse(sumList.stdout).unsupportedSumCarriers[0].reason, /resolved pub type JetOutcome.*seed-only/);
  // A declaration-proven Result alias is the existing selected-out codec,
  // not a named opaque carrier. Resolve actual and fixed payloads in their
  // own modules, including renamed and glob imports of the alias.
  const serviceAlias = `
pub mod host {
    #[derive(Clone, PartialEq)]
    pub struct JetAliasError { pub message: String }
    pub type JetUiServiceResult<T> = Result<T, JetAliasError>;
}
pub mod renamed {
    use crate::host::JetUiServiceResult as Service;
    pub fn jet_fixture_alias() -> Service<crate::JetFixture> { unreachable!() }
}
pub mod globbed {
    use crate::host::*;
    pub fn jet_fixture_alias_glob() -> JetUiServiceResult<crate::JetFixture> { unreachable!() }
}
`;
  put("crates/jet-foundation/src/Syntax/core_calls.rs", registry('CoreCallRecord::new("core.fixture", "alias", "renamed::jet_fixture_alias", true)\nCoreCallRecord::new("core.fixture", "alias_glob", "globbed::jet_fixture_alias_glob", true)\n'));
  put("runtime.rs", declarations + serviceAlias);
  success(run());
  success(run(true));
  const aliasRust = readFileSync(join(fixture, "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs"), "utf8");
  const aliasJet = readFileSync(join(fixture, "Compiler/JetBackend/Source/Lower/RuntimeRoutes.jet"), "utf8");
  assert.match(aliasRust, /jet_fixture_alias\(ok: \*mut \*mut crate::JetFixture, err: \*mut \*mut crate::host::JetAliasError\) -> i64/);
  assert.match(aliasRust, /jet_fixture_alias_glob\(ok: \*mut \*mut crate::JetFixture, err: \*mut \*mut crate::host::JetAliasError\) -> i64/);
  assert.ok(aliasRust.includes("Ok(value) => { unsafe { ok.write(Box::into_raw(Box::new(value))) }; 1 }"));
  assert.ok(aliasRust.includes("Err(error) => { unsafe { err.write(Box::into_raw(Box::new(error))) }; 0 }"));
  assert.ok(aliasJet.includes('LowerRouteResult.Result{ok: LowerRouteCarrier.Handle{name: "JetFixture"}, err: LowerRouteCarrier.Handle{name: "JetAliasError"}}'));
  assert.ok(!aliasRust.includes("jet_rt_handle_drop_jet_ui_service_result"), "a Result alias must never invent ownership exports");
  put("crates/jet-foundation/src/Syntax/core_calls.rs", registry('CoreCallRecord::new("core.fixture", "tuple", "jet_fixture_tuple", true)\n'));
  put("runtime.rs", declarations + "#[derive(Clone,PartialEq)]\npub struct JetOutcome<T,E> { pub value: T, pub error: E }\npub fn jet_fixture_tuple(value: crate::JetOutcome<f64,bool>) -> f64 { 0.0 }\n");
  success(run());
  assert.ok(readFileSync(join(fixture, "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs"), "utf8").includes("fn jet_fixture_tuple("), "crate::JetOutcome nominal must retain its handle route");
  put("runtime.rs", declarations + sumAlias + "#[derive(Clone)]\npub struct JetOutcome<T,E> { pub value: T, pub error: E }\npub fn jet_fixture_tuple() -> (JetOutcome<jet_foundation::Numeric::JetInt,JetAbsent>, f64) { unreachable!() }\n");
  const ambiguousSum = run();
  assert.notEqual(ambiguousSum.status, 0);
  assert.match(ambiguousSum.stderr, /JetOutcome.*ambiguous/);
  put("crates/jet-foundation/src/Syntax/core_calls.rs", registry('CoreCallRecord::new("core.fixture", "arc", "jet_fixture_arc", true)\n'));
  const callback = "dyn Fn()->Result<JetHTTPResponse,JetHTTPError>+Send+Sync+'static,";
  for (const [imports, type] of [["", "std::sync::Arc"], ["use std::sync::Arc;\n", "Arc"], ["use std::sync::*;\n", "Arc"]]) {
    put("runtime.rs", declarations + imports + `pub struct JetHTTPResponse;\npub struct JetHTTPError;\npub fn jet_fixture_arc(value: ${type}<${callback}>) -> f64 { 0.0 }\n`);
    success(run());
    const closureList = spawnSync(process.execPath, [join(fixture, "Tools/agent/gen-c-abi-routes.mjs"), join(fixture, "runtime.rs"), "--native-list-only"], { encoding: "utf8" });
    success(closureList);
    const closure = JSON.parse(closureList.stdout).unsupportedRustCarriers;
    assert.ok(closure.some((row) => row.qualified.startsWith("std::sync::Arc<") && row.reason.includes("no C callback codec")));
  }
  put("runtime.rs", declarations + "#[derive(Clone,PartialEq)]\npub struct Arc<T> { pub value: T }\npub fn jet_fixture_arc(value: crate::Arc<f64>) -> f64 { 0.0 }\n");
  success(run());
  assert.ok(readFileSync(join(fixture, "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs"), "utf8").includes("fn jet_fixture_arc("), "crate-local Arc must remain nominal");
  put("crates/jet-foundation/src/Syntax/core_calls.rs", registry('CoreCallRecord::new("core.fixture", "box", "jet_fixture_box", true)\n'));
  put("runtime.rs", declarations + "pub fn jet_fixture_box(value: Box<dyn FnOnce()>) -> f64 { 0.0 }\n");
  success(run());
  const boxList = spawnSync(process.execPath, [join(fixture, "Tools/agent/gen-c-abi-routes.mjs"), join(fixture, "runtime.rs"), "--native-list-only"], { encoding: "utf8" });
  success(boxList);
  assert.ok(JSON.parse(boxList.stdout).unsupportedRustCarriers.some((row) => row.qualified === "std::boxed::Box<dynFnOnce()>"));
  put("runtime.rs", declarations + "#[derive(Clone,PartialEq)]\npub struct Box<T> { pub value: T }\npub fn jet_fixture_box(value: Box<f64>) -> f64 { 0.0 }\n");
  success(run());
  assert.ok(readFileSync(join(fixture, "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs"), "utf8").includes(`jet_rt_record_new_${runtimeAbiAtom(recordInstanceName("Box", ["f64"]))}`), "local Box shadows the Rust prelude");
  for (const [prefix, type] of [["#![no_implicit_prelude]\n", "Box"], ["#![no_std]\n", "Box"], ["", "crate::Box"]]) {
    put("runtime.rs", prefix + declarations + `pub fn jet_fixture_box(value: ${type}<f64>) -> f64 { 0.0 }\n`);
    assert.match(run().stderr, /unresolved or ambiguous declaration/);
  }
  put("crates/jet-foundation/src/Syntax/core_calls.rs", registry('CoreCallRecord::new("core.fixture", "ui", "jet_fixture_ui", true)\n'));
  put("runtime.rs", declarations + "pub struct JetUiHostError;\npub type JetUiServiceResult<T> = Result<T, JetUiHostError>;\npub fn jet_fixture_ui() -> JetUiServiceResult<Option<String>> { unreachable!() }\n");
  success(run());
  const uiList = spawnSync(process.execPath, [join(fixture, "Tools/agent/gen-c-abi-routes.mjs"), join(fixture, "runtime.rs"), "--native-list-only"], { encoding: "utf8" });
  success(uiList);
  assert.ok(JSON.parse(uiList.stdout).unsupportedSumCarriers.some((row) => row.reason.includes("pub type JetUiServiceResult<T> = Result<T, JetUiHostError>")));
  put("crates/jet-foundation/src/Syntax/core_calls.rs", registry('CoreCallRecord::new("core.fixture", "smtp", "jet_fixture_smtp", true)\n'));
  put("runtime.rs", declarations + "pub struct SMTPConfig<S: AsRef<[u8]> = Vec<u8>> { pub secret: S }\npub fn jet_fixture_smtp(value: SMTPConfig<Vec<u8>>) -> f64 { 0.0 }\n");
  success(run());
  const smtpList = spawnSync(process.execPath, [join(fixture, "Tools/agent/gen-c-abi-routes.mjs"), join(fixture, "runtime.rs"), "--native-list-only"], { encoding: "utf8" });
  success(smtpList);
  assert.ok(JSON.parse(smtpList.stdout).unsupportedGenericCarriers.some((row) => row.type === "SMTPConfig<Vec<u8>>" && Number.isInteger(row.declaration.at)));
  put("runtime.rs", declarations + "pub struct SMTPConfig<S = Vec<u8>> { pub secret: S }\npub fn jet_fixture_smtp(value: SMTPConfig<Vec<u8>,String>) -> f64 { 0.0 }\n");
  assert.match(run().stderr, /expected 0\.\.1 declaration arguments, got 2/);
  put("crates/jet-foundation/src/Syntax/core_calls.rs", registry('CoreCallRecord::new("core.fixture", "unit", "jet_fixture_unit", true)\n'));
  const unitRecord = "#[derive(Clone)]\npub enum JetLoadable<T:Clone,E:Clone> { Idle, Loading, Loaded(T), Failed(E) }\npub fn jet_fixture_unit() -> JetLoadable<(),()> { unreachable!() }\n";
  put("runtime.rs", declarations + unitRecord);
  success(run());
  assert.ok(readFileSync(join(fixture, "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs"), "utf8").includes("result type JetLoadable<(),()>"));
  const unitList = spawnSync(process.execPath, [join(fixture, "Tools/agent/gen-c-abi-routes.mjs"), join(fixture, "runtime.rs"), "--native-list-only"], { encoding: "utf8" });
  success(unitList);
  assert.match(JSON.parse(unitList.stdout).unsupportedGenericCarriers[0].reason, /seed TYPE-Unit specialization unsupported/);
  put("runtime.rs", declarations + unitRecord.replaceAll("JetLoadable", "__jet_Loadable"));
  const sourceUnit = run();
  assert.notEqual(sourceUnit.status, 0);
  assert.match(sourceUnit.stderr, /SOURCE TYPE-Unit specialization requires a typed Unit codec/);
  put("crates/jet-foundation/src/Syntax/core_calls.rs", registry('CoreCallRecord::new("core.fixture", "receiver", "jet_fixture_receiver", true)\n'));
  const receiver = "#[derive(Clone)]\npub struct JetReceiver<T:Clone> { pub value: T }\npub fn jet_fixture_receiver() -> JetReceiver<i64> { unreachable!() }\n";
  put("runtime.rs", declarations + receiver);
  success(run());
  assert.ok(readFileSync(join(fixture, "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs"), "utf8").includes("result type JetReceiver<i64>"));
  const rawList = spawnSync(process.execPath, [join(fixture, "Tools/agent/gen-c-abi-routes.mjs"), join(fixture, "runtime.rs"), "--native-list-only"], { encoding: "utf8" });
  success(rawList);
  assert.match(JSON.parse(rawList.stdout).unsupportedGenericCarriers[0].reason, /seed raw-machine TYPE specialization unsupported/);
  put("runtime.rs", declarations + receiver.replaceAll("JetReceiver", "__jet_Receiver"));
  const sourceRaw = run();
  assert.notEqual(sourceRaw.status, 0);
  assert.match(sourceRaw.stderr, /SOURCE raw-machine TYPE specialization requires a typed identity\/record codec/);
  // List-only reports all classification failures, keeps exit 1, writes no
  // output table, and never presents incomplete descriptors as admissions.
  put("crates/jet-foundation/src/Syntax/core_calls.rs", registry('CoreCallRecord::new("core.fixture", "receiver", "jet_fixture_receiver", true)\nCoreCallRecord::new("core.fixture", "missing", "jet_fixture_missing", true)\n'));
  put("runtime.rs", declarations + receiver.replaceAll("JetReceiver", "__jet_Receiver") + "pub fn jet_fixture_missing() -> MissingRecord<f64> { unreachable!() }\n");
  const beforeErrorsRust = readFileSync(join(fixture, "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs"), "utf8");
  const beforeErrorsJet = readFileSync(join(fixture, "Compiler/JetBackend/Source/Lower/RuntimeRoutes.jet"), "utf8");
  const allErrors = spawnSync(process.execPath, [join(fixture, "Tools/agent/gen-c-abi-routes.mjs"), join(fixture, "runtime.rs"), "--native-list-only"], { encoding: "utf8" });
  assert.equal(allErrors.status, 1, allErrors.stderr + allErrors.stdout);
  const packet = JSON.parse(allErrors.stdout);
  assert.equal(packet.mode, "native-pack-classification-errors");
  assert.equal(packet.nativeTypes, undefined);
  assert.equal(packet.nativeExports, undefined);
  for (const type of ["MissingRecord<f64>", "__jet_Receiver<i64>"]) {
    assert.ok(packet.classificationErrors.some((error) => error.type === type && Number.isInteger(error.site.consumer.offset)));
  }
  assert.equal(readFileSync(join(fixture, "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs"), "utf8"), beforeErrorsRust);
  assert.equal(readFileSync(join(fixture, "Compiler/JetBackend/Source/Lower/RuntimeRoutes.jet"), "utf8"), beforeErrorsJet);
  put("crates/jet-foundation/src/Syntax/core_calls.rs", registry(""));
  put("runtime.rs", declarations);
  success(run());
  success(run(true));
  const baselineRust = readFileSync(join(fixture, "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs"), "utf8");
  const baselineJet = readFileSync(join(fixture, "Compiler/JetBackend/Source/Lower/RuntimeRoutes.jet"), "utf8");
  // List-only is the real native descriptor pass and must leave BOTH outputs
  // byte-identical, even when this source still has seed-only runtime owners.
  const listed = spawnSync(process.execPath, [join(fixture, "Tools/agent/gen-c-abi-routes.mjs"), join(fixture, "runtime.rs"), "--native-list-only"], { encoding: "utf8" });
  success(listed);
  const inventory = JSON.parse(listed.stdout);
  assert.equal(inventory.mode, "native-pack-descriptors");
  assert.match(inventory.provenance.runtimeSha256, /^[0-9a-f]{64}$/u);
  assert.ok(inventory.seedHelperNames.some((name) => name === "jet_rt_handle_drop_jet_fixture"));
  assert.deepEqual(inventory.nativeExports, []);
  assert.equal(readFileSync(join(fixture, "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs"), "utf8"), baselineRust);
  assert.equal(readFileSync(join(fixture, "Compiler/JetBackend/Source/Lower/RuntimeRoutes.jet"), "utf8"), baselineJet);
  // Ownership is per exact symbol: proven Jet providers replace their bodies,
  // while unported helpers stay excluded from native packs, not fabricated.
  const providedGlue = "#Export(c) fn jet_rt_handle_drop_jet_fixture(value: I64) {}\n#Export(c) fn jet_rt_handle_eq_jet_fixture(left: I64, right: I64) -> Bool { true }\n";
  put("Compiler/JetRuntime/Source/Fixture.jet", "#Export(c) fn jet_fixture_scalar(value: Float) -> Float { value }\n" + providedGlue);
  put("Compiler/JetRuntime/exports.list", "jet_fixture_scalar\njet_rt_handle_drop_jet_fixture\njet_rt_handle_eq_jet_fixture\n");
  const partialGlue = run();
  success(partialGlue);
  const partialRust = readFileSync(join(fixture, "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs"), "utf8");
  assert.ok(!partialRust.includes("fn jet_rt_handle_drop_jet_fixture("));
  assert.ok(partialRust.includes('#[cfg(not(jet_native_pack))]\n    #[no_mangle]\n    pub extern "C" fn jet_rt_handle_clone_jet_fixture('));
  const completeGlue = providedGlue +
    "#Export(c) fn jet_rt_handle_clone_jet_fixture(value: I64) -> I64 { value }\n" +
    "#Export(c) fn jet_rt_record_get_jet_fixture_value(value: I64) -> Float { Float{0} }\n" +
    "#Export(c) fn jet_rt_record_new_jet_fixture(value: Float) -> I64 { I64{0} }\n" +
    "#Export(c) fn jet_fixture_plain(value: I64) -> I64 { value }\n";
  const completeNames = [...completeGlue.matchAll(/fn (\w+)\(/g)].map((match) => match[1]);
  const composeGlue = (names) => {
    put("Compiler/JetRuntime/Source/Fixture.jet", "#Export(c) fn jet_fixture_scalar(value: Float) -> Float { value }\n" +
      completeGlue.split("\n").filter((line) => names.some((name) => line.includes(`fn ${name}(`))).join("\n"));
    put("Compiler/JetRuntime/exports.list", ["jet_fixture_scalar", ...names].join("\n") + "\n");
  };
  // Moving lifecycle helpers does not classify the remaining record/route
  // bodies as native. Each unprovided body retains its seed-only gate.
  composeGlue(completeNames.filter((name) => name.startsWith("jet_rt_handle_")));
  success(run());
  assert.ok(readFileSync(join(fixture, "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs"), "utf8").includes('#[cfg(not(jet_native_pack))]\n    #[no_mangle]\n    pub extern "C" fn jet_rt_record_get_jet_fixture_value('));
  composeGlue(completeNames.filter((name) => name !== "jet_fixture_plain"));
  success(run());
  assert.ok(readFileSync(join(fixture, "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs"), "utf8").includes('#[cfg(not(jet_native_pack))]\n    #[no_mangle]\n    pub extern "C" fn jet_fixture_plain('));
  composeGlue(completeNames);
  success(run());
  success(run(true));
  const ownedRust = readFileSync(join(fixture, "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs"), "utf8");
  assert.ok(!ownedRust.includes("fn jet_rt_handle_clone_jet_fixture("));
  for (const name of ["jet_rt_record_get_jet_fixture_value", "jet_rt_record_new_jet_fixture", "jet_fixture_plain"]) {
    assert.ok(ownedRust.includes(`#[cfg(not(jet_native_pack))]\n    #[no_mangle]\n    pub extern "C" fn ${name}(`));
  }
  put("Compiler/JetRuntime/exports.list", "jet_fixture_scalar\njet_rt_handle_drop_jet_fixture\njet_rt_handle_eq_jet_fixture\n");
  put("Compiler/JetRuntime/Source/Fixture.jet", "#Export(c) fn jet_fixture_scalar(value: Float) -> Float { value }\n" + providedGlue.replace("left: I64", "left: Float"));
  const mismatchedGlue = run();
  assert.notEqual(mismatchedGlue.status, 0);
  assert.match(mismatchedGlue.stderr, /jet_rt_handle_eq_jet_fixture.*does not match C ABI/);
  put("Compiler/JetRuntime/Source/Fixture.jet", "#Export(c) fn jet_fixture_scalar(value: Float) -> Float { value }\n");
  put("Compiler/JetRuntime/exports.list", "jet_fixture_scalar\n");
  // Single-Int records cross scalar routes as exact words, without a Box.
  // A mutable borrow or a different declaration keeps its existing carrier.
  put("crates/jet-foundation/src/Syntax/core_calls.rs", registry(
    ["scalar_record", "borrowed_record", "mutable_record", "wide_record", "shadow_record"]
      .map((name) => `CoreCallRecord::new("core.fixture", "${name}", "jet_fixture_${name}", true)`).join("\n")));
  put("runtime.rs", declarations + `
#[derive(Clone, PartialEq)] pub struct JetScalar { pub value: i64 }
pub fn jet_fixture_scalar_record(value: JetScalar) -> JetScalar { value }
pub fn jet_fixture_borrowed_record(value: &JetScalar) -> i64 { value.value }
pub fn jet_fixture_mutable_record(value: &mut JetScalar) {}
#[derive(Clone)] pub struct JetWide { pub first: i64, pub second: i64 }
pub fn jet_fixture_wide_record(value: JetWide) -> JetWide { value }
pub mod shadow {
  #[derive(Clone)] pub struct JetScalar { pub value: bool }
  pub fn jet_fixture_shadow_record(value: JetScalar) -> JetScalar { value }
}
`);
  success(run());
  const scalarRust = readFileSync(join(fixture, "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs"), "utf8");
  const scalarJet = readFileSync(join(fixture, "Compiler/JetBackend/Source/Lower/RuntimeRoutes.jet"), "utf8");
  assert.match(scalarRust, /fn jet_fixture_scalar_record\(a0: i64\) -> i64/);
  assert.ok(scalarRust.includes("crate::JetScalar { value: native(a0) }"));
  assert.ok(scalarRust.includes("crate::jet_std::jet_int_from_i64((crate::jet_fixture_scalar_record"));
  assert.match(scalarRust, /fn jet_fixture_borrowed_record\(a0: i64\)/);
  assert.ok(scalarRust.includes("&crate::JetScalar { value: native(a0) }"));
  assert.match(scalarRust, /fn jet_fixture_mutable_record\(a0: \*mut crate::JetScalar\)/);
  assert.ok(scalarJet.includes('LowerWordRecord{runtime: "JetScalar", field: "value"}'));
  assert.ok(!scalarJet.includes('LowerWordRecord{runtime: "JetWide"'));
  assert.ok(!scalarJet.includes('LowerWordRecord{runtime: "shadow::JetScalar"'));
  const genericRows = `CoreCallRecord::new("core.fixture", "measurement", "jet_fixture_measurement", true)\nCoreCallRecord::new("core.fixture", "pair", "jet_fixture_pair", true)\n`;
  put("crates/jet-foundation/src/Syntax/core_calls.rs", registry(genericRows));
  put("runtime.rs", declarations + `pub fn jet_fixture_measurement(value: JetMeasurement<f64>) -> JetMeasurement<f64> { value }\npub fn jet_fixture_pair(value: JetPair<f64,bool>) -> JetPair<f64,bool> { value }\n`);
  success(run());
  success(run(true));
  const rust = readFileSync(join(fixture, "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs"), "utf8");
  const jet = readFileSync(join(fixture, "Compiler/JetBackend/Source/Lower/RuntimeRoutes.jet"), "utf8");
  assert.ok(jet.includes(`LowerRecordSchema{jet_name: "${measurement}", variants: "", fields: "value=Float,uncertainty=Float"}`));
  assert.ok(jet.includes(`LowerRecordSchema{jet_name: "${pair}", variants: "", fields: "first=Float,second=Flag"}`));
  // Measurement is source-owned; a seed JetMeasurement carrier is not that
  // authored declaration and must not manufacture a canonical layout alias.
  assert.ok(!jet.includes(`jet_name: "<corelib>/Core/units::Core/units/units.jet::Measurement<Float>", runtime: "${measurement}"`));
  assert.ok(!jet.includes('LowerNativeType{jet_name: "<corelib>/Core/units::Core/units/units.jet::Measurement<Float>"'));
  assert.ok(jet.includes(`jet_name: "<corelib>/Core/units::Core/units/units.jet::Pair<Float,Bool>", runtime: "${pair}"`));
  assert.ok(rust.includes(`jet_rt_record_new_${runtimeAbiAtom(measurement)}(m0: f64, m1: f64)`));
  assert.ok(rust.includes("crate::JetMeasurement::<f64> { value: m0, uncertainty: m1 }"));
  // Existing schema, handle and helper definitions remain byte-identical.
  for (const row of baselineJet.split("\n").filter((line) => /Lower(?:RuntimeHandle|RecordSchema)\{/.test(line))) assert.ok(jet.includes(row));
  const helper = (source) => [...source.matchAll(/    #\[no_mangle\]\n    pub extern "C" fn jet_rt_(?:handle|record)_\w*jet_fixture\w*\([^]*?\n    \}/g)].map((match) => match[0]);
  assert.deepEqual(helper(rust), helper(baselineRust));
  assert.ok(helper(baselineRust).length > 0, "non-generic helper comparison must exercise real definitions");
  // No source declaration or Rust carrier is needed for an opaque provider.
  put("Compiler/JetRuntime/Source/Fixture.jet", "#Export(c) fn jet_fixture_scalar(value: Float) -> Float { value }\n" + pipeDefinitions);
  put("Compiler/JetRuntime/exports.list", ["jet_fixture_scalar", ...pipeAbi.keys()].join("\n") + "\n");
  success(run());
  success(run(true));
  const pipeJet = readFileSync(join(fixture, "Compiler/JetBackend/Source/Lower/RuntimeRoutes.jet"), "utf8");
  const pipeRust = readFileSync(join(fixture, "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs"), "utf8");
  for (const type of ["ProcessStdoutStream", "ProcessStderrStream"]) {
    assert.ok(pipeJet.includes(`jet_name: "<corelib>/Core/process::Core/process/process.jet::${type}", runtime: "${type}", clone: true`));
    assert.ok(pipeJet.includes(`jet_name: "${type}", runtime: "${type}", clone: true`));
    assert.ok(!pipeRust.includes(type), "provider must not invent a Rust carrier or adapter");
    assert.ok(!pipeJet.includes(`LowerRecordSchema{jet_name: "${type}"`));
    assert.ok(!pipeJet.includes(`LowerNativeType{jet_name: "${type}"`));
  }
  const lookup = pipeJet.slice(pipeJet.indexOf("fn lower_runtime_handle("));
  const seedRow = lookup.indexOf("lower_key_row(LOWER_RUNTIME_HANDLE_KEYS"), providerRow = lookup.indexOf("lower_key_row(LOWER_NATIVE_PROVIDER_HANDLE_KEYS");
  assert.ok(seedRow >= 0 && providerRow > seedRow, "seed lookup must never override an existing Rust row");
  put("Compiler/JetRuntime/Source/Fixture.jet", "#Export(c) fn jet_fixture_scalar(value: Float) -> Float { value }\n");
  put("Compiler/JetRuntime/exports.list", "jet_fixture_scalar\n");
  const bitsDefinitions = "#Export(c) fn jet_rt_native_bits_new() -> I64 { I64{0} }\n#Export(c) fn jet_rt_native_bits_clear(bits: I64) {}\n";
  put("Compiler/JetRuntime/Source/Fixture.jet", "#Export(c) fn jet_fixture_scalar(value: Float) -> Float { value }\n" + bitsDefinitions);
  put("Compiler/JetRuntime/exports.list", "jet_fixture_scalar\njet_rt_native_bits_new\njet_rt_native_bits_clear\n");
  success(run());
  success(run(true));
  const bitsJet = readFileSync(join(fixture, "Compiler/JetBackend/Source/Lower/RuntimeRoutes.jet"), "utf8");
  const bitsRust = readFileSync(join(fixture, "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs"), "utf8");
  assert.ok(bitsJet.includes('export: "jet_rt_native_bits_new", native_lists: true, params: [LowerRouteCarrier]{}, result: LowerRouteResult.Plain{carrier: LowerRouteCarrier.Handle{name: "JetBitSet"}}'));
  assert.ok(bitsJet.includes('export: "jet_rt_native_bits_clear", native_lists: true, params: [LowerRouteCarrier]{LowerRouteCarrier.HandleMut{name: "JetBitSet"}}, result: LowerRouteResult.Plain{carrier: LowerRouteCarrier.Unit}'));
  assert.ok(!bitsRust.includes("fn jet_rt_native_bits_"), "native-only providers must not invent seed kernels");
  put("Compiler/JetRuntime/Source/Fixture.jet", "#Export(c) fn jet_fixture_scalar(value: Float) -> Float { value }\n" + bitsDefinitions.replace("bits: I64", "bits: Float"));
  assert.match(run().stderr, /bits_clear.*does not match C ABI/);
  put("Compiler/JetRuntime/Source/Fixture.jet", "#Export(c) fn jet_fixture_scalar(value: Float) -> Float { value }\n");
  put("Compiler/JetRuntime/exports.list", "jet_fixture_scalar\n");
  const weakDefinitions = [
    "#Export(c) fn jet_rt_native_shared_downgrade(shared: I64) -> I64 { shared }",
    "#Export(c) fn jet_rt_native_shared_weak_upgrade(weak: I64, out: I64) -> Bool { true }",
    "#Export(c) fn jet_rt_native_shared_weak_drop(weak: I64) {}",
  ].join("\n") + "\n";
  const weakNames = [...weakDefinitions.matchAll(/fn (\w+)\(/g)].map((match) => match[1]);
  put("Compiler/JetRuntime/Source/Fixture.jet", "#Export(c) fn jet_fixture_scalar(value: Float) -> Float { value }\n" + weakDefinitions);
  put("Compiler/JetRuntime/exports.list", ["jet_fixture_scalar", ...weakNames].join("\n") + "\n");
  success(run());
  success(run(true));
  const weakJet = readFileSync(join(fixture, "Compiler/JetBackend/Source/Lower/RuntimeRoutes.jet"), "utf8");
  const weakRust = readFileSync(join(fixture, "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs"), "utf8");
  for (const name of weakNames) assert.ok(!weakRust.includes(`fn ${name}(`), "native weak exports must never synthesize Rust provider bodies");
  assert.ok(weakJet.includes('export: "jet_rt_native_shared_downgrade", native_lists: true, params: [LowerRouteCarrier]{LowerRouteCarrier.Word}, result: LowerRouteResult.Plain{carrier: LowerRouteCarrier.Word}'));
  assert.ok(weakJet.includes('export: "jet_rt_native_shared_weak_upgrade", native_lists: true, params: [LowerRouteCarrier]{LowerRouteCarrier.Word, LowerRouteCarrier.Word}, result: LowerRouteResult.Plain{carrier: LowerRouteCarrier.Flag}'));
  assert.ok(weakJet.includes('export: "jet_rt_native_shared_weak_drop", native_lists: true, params: [LowerRouteCarrier]{LowerRouteCarrier.Word}, result: LowerRouteResult.Plain{carrier: LowerRouteCarrier.Unit}'));
  // Source-signature fixtures for the accepted physical owner family. These
  // exercise schema validation only, not the provider's ownership behavior.
  const physicalShared = [
    ["owner_strong_count", 1, "I64"], ["owner_pin", 2, "Bool"],
    ["pin_drop", 1, ""], ["owner_alias", 2, "Bool"],
    ["alias_token", 1, "I64"], ["alias_drop", 1, ""],
    ["guard_acquire", 2, "I64"], ["guard_clone", 1, "I64"],
    ["guard_held", 1, "Bool"], ["guard_root_ptr", 1, "I64"],
    ["guard_revision", 1, "I64"], ["guard_mark_dirty", 1, ""],
    ["guard_publish_and_release", 1, ""], ["guard_abort_release", 1, ""],
    ["guard_reacquire_and_refresh", 2, "Bool"], ["guard_drop", 1, ""],
    ["guard_wait_once", 2, "I64"],
  ];
  const physicalNames = physicalShared.map(([op]) => `jet_rt_shared_${runtimeAbiAtom(op)}`);
  const physicalDefinitions = physicalShared.map(([op, arity, result]) =>
    `#Export(c) fn jet_rt_shared_${runtimeAbiAtom(op)}(${Array.from({length: arity}, (_, i) => `a${i}: I64`).join(", ")})${result ? ` -> ${result}` : ""} {${result === "I64" ? " a0 " : result === "Bool" ? " a0 != I64{0} " : ""}}`).join("\n");
  put("Compiler/JetRuntime/Source/Fixture.jet", "#Export(c) fn jet_fixture_scalar(value: Float) -> Float { value }\n" + weakDefinitions + physicalDefinitions);
  put("Compiler/JetRuntime/exports.list", ["jet_fixture_scalar", ...weakNames, ...physicalNames].join("\n") + "\n");
  success(run());
  const physicalJet = readFileSync(join(fixture, "Compiler/JetBackend/Source/Lower/RuntimeRoutes.jet"), "utf8");
  const physicalRust = readFileSync(join(fixture, "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs"), "utf8");
  for (const [op, arity, result] of physicalShared) {
    const name = `jet_rt_shared_${runtimeAbiAtom(op)}`;
    assert.ok(!physicalRust.includes(`fn ${name}(`));
    assert.ok(physicalJet.includes(`export: "${name}", native_lists: true, params: [LowerRouteCarrier]{${Array.from({length: arity}, () => "LowerRouteCarrier.Word").join(", ")}}, result: LowerRouteResult.Plain{carrier: LowerRouteCarrier.${result === "I64" ? "Word" : result === "Bool" ? "Flag" : "Unit"}}`));
  }
  put("Compiler/JetRuntime/Source/Fixture.jet", "#Export(c) fn jet_fixture_scalar(value: Float) -> Float { value }\n" + weakDefinitions + physicalDefinitions.replace("a1: I64", "a1: Float"));
  assert.match(run().stderr, /owner_pin.*does not match C ABI/);
  put("Compiler/JetRuntime/Source/Fixture.jet", "#Export(c) fn jet_fixture_scalar(value: Float) -> Float { value }\n" + weakDefinitions);
  put("Compiler/JetRuntime/exports.list", ["jet_fixture_scalar", ...weakNames].join("\n") + "\n");
  success(run());
  assert.ok(!readFileSync(join(fixture, "Compiler/JetBackend/Source/Lower/RuntimeRoutes.jet"), "utf8").includes('export: "jet_rt_shared_owner_pin"'));
  put("Compiler/JetRuntime/Source/Fixture.jet", "#Export(c) fn jet_fixture_scalar(value: Float) -> Float { value }\n" + weakDefinitions.replace("out: I64", "out: Float"));
  assert.match(run().stderr, /weak_upgrade.*does not match C ABI/);
  put("Compiler/JetRuntime/Source/Fixture.jet", "#Export(c) fn jet_fixture_scalar(value: Float) -> Float { value }\n");
  put("Compiler/JetRuntime/exports.list", "jet_fixture_scalar\n");
  // Source-owned generics have actual mangled declarations and private,
  // mangled fields, not a historical native carrier name or a C route.
  const sampleKey = "<corelib>/Core/fixture::Core/fixture/fixture.jet::Sample";
  const pairKey = "<corelib>/Core/fixture::Core/fixture/fixture.jet::Pair";
  const scalarKey = "<corelib>/Core/fixture::Core/fixture/fixture.jet::Scalar";
  const markerKey = "<corelib>/Core/fixture::Core/fixture/fixture.jet::Marker";
  const scalarSource = generatedRecordPath(scalarKey), markerSource = generatedRecordPath(markerKey);
  const sampleSource = generatedRecordPath(sampleKey), pairSource = generatedRecordPath(pairKey);
  put("Core/fixture/fixture.jet", "struct Sample<T> { value: T, uncertainty: T }\nstruct Pair<A,B> { first: A, second: B }\nstruct Scalar { value: Float }\nstruct Marker {}\n");
  put("runtime.rs", declarations + `
#[derive(Clone)]
pub struct ${sampleSource}<T> { __jet_value: T, __jet_uncertainty: T }
#[derive(Clone)]
pub struct ${pairSource}<A,B> { __jet_first: A, __jet_second: B }
#[derive(Clone)]
pub struct ${scalarSource} { __jet_value: f64 }
#[derive(Clone)]
pub struct ${markerSource} {}
pub fn source_scalar(value: f64) -> ${scalarSource} { ${scalarSource} { __jet_value: value } }
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
  assert.ok(sourceJet.includes(`LowerNativeExport{symbol: "jet_rt_handle_clone_${runtimeAbiAtom(sampleInstance)}", jet_name: "${sampleKey}<Float>", op: "handle.clone", member: ""}`));
  assert.ok(sourceJet.includes(`LowerNativeExport{symbol: "jet_rt_record_new_${runtimeAbiAtom(sampleInstance)}", jet_name: "${sampleKey}<Float>", op: "record.new", member: ""}`));
  assert.ok(sourceJet.includes(`LowerNativeExport{symbol: "jet_rt_record_get_${runtimeAbiAtom(sampleInstance)}_uncertainty", jet_name: "${sampleKey}<Float>", op: "record.get", member: "uncertainty"}`));
  assert.ok(sourceRust.includes(`#[cfg(not(jet_native_pack))]\n    #[no_mangle]\n    pub extern "C" fn jet_rt_record_new_${runtimeAbiAtom(sampleInstance)}(`));
  const treeKey = "<corelib>/Core/fixture::Core/fixture/fixture.jet::Tree";
  const treeSource = generatedRecordPath(treeKey);
  put("Core/fixture/fixture.jet", readFileSync(join(fixture, "Core/fixture/fixture.jet"), "utf8") + "enum Tree { Empty, Bytes([U8]), Number(Float) }\n");
  put("runtime.rs", readFileSync(join(fixture, "runtime.rs"), "utf8") + `
#[derive(Clone)]
pub enum ${treeSource} { __jet_Empty, __jet_Bytes(Vec<u8>), __jet_Number(f64) }
pub fn source_tree() -> ${treeSource} { ${treeSource}::__jet_Empty }
`);
  success(run());
  success(run(true));
  const treeJet = readFileSync(join(fixture, "Compiler/JetBackend/Source/Lower/RuntimeRoutes.jet"), "utf8");
  assert.ok(treeJet.includes(`LowerNativeExport{symbol: "jet_rt_record_new_${runtimeAbiAtom(treeSource)}_bytes", jet_name: "${treeKey}", op: "record.new", member: "Bytes"}`));
  assert.ok(treeJet.includes(`LowerNativeExport{symbol: "jet_rt_record_seq_list_byte_get", jet_name: "${treeKey}", op: "record.seq.get", member: "List.Byte"}`));
  assert.ok(!treeJet.includes('symbol: "jet_rt_record_seq_list_byte_key"'), "descriptors cannot invent a C symbol absent from the seed ABI");
  assert.ok(sourceRust.includes(`${sampleSource}::<f64> { __jet_value: m0, __jet_uncertainty: m1 }`));
  assert.ok(sourceJet.includes(`LowerNativeType{jet_name: "${scalarKey}", declaration: "${scalarKey}", arguments: ""}`));
  assert.ok(sourceJet.includes(`LowerRecordSchema{jet_name: "${scalarSource}", variants: "", fields: "value=Float"}`));
  assert.ok(!sourceJet.includes(markerSource), "an unused marker declaration is not an ownership proof");
  assert.ok(sourceRust.includes(`${scalarSource} { __jet_value: m0 }`));
  put("runtime.rs", declarations + "pub fn jet_fixture_measurement(value: JetMeasurement<Unbound>) -> JetMeasurement<f64> { value }\n");
  const unresolved = run();
  assert.notEqual(unresolved.status, 0);
  assert.match(unresolved.stderr, /JetMeasurement<Unbound>.*unresolved or unsupported argument Unbound/);
  for (const argument of ["u8", "i64", "u64", "f32"]) {
    put("runtime.rs", declarations + `pub fn jet_fixture_measurement(value: JetMeasurement<${argument}>) -> JetMeasurement<f64> { value }\n`);
    success(run());
    const unsupportedList = spawnSync(process.execPath, [join(fixture, "Tools/agent/gen-c-abi-routes.mjs"), join(fixture, "runtime.rs"), "--native-list-only"], { encoding: "utf8" });
    success(unsupportedList);
    const unsupportedSeed = JSON.parse(unsupportedList.stdout);
    assert.ok(unsupportedSeed.unsupportedGenericCarriers.some((row) => row.type === `JetMeasurement<${argument}>` && row.reason.includes("seed raw-machine TYPE specialization unsupported")));
    assert.ok(!unsupportedSeed.nativeTypes.some((row) => row.runtime === recordInstanceName("JetMeasurement", [argument])), `${argument} must not create an unmatched Lower row`);
    put("runtime.rs", declarations.replaceAll("JetMeasurement", "__jet_Measurement") + `pub fn jet_fixture_measurement(value: __jet_Measurement<${argument}>) -> __jet_Measurement<f64> { value }\n`);
    const unsupported = run();
    assert.notEqual(unsupported.status, 0, `${argument} SOURCE specialization requires its own codec`);
    assert.match(unsupported.stderr, /SOURCE raw-machine TYPE specialization requires a typed identity\/record codec/);
  }
  put("runtime.rs", declarations + "pub fn jet_fixture_measurement(value: JetMeasurement<Vec<f64>>) -> JetMeasurement<f64> { value }\n");
  success(run());
  const nestedList = spawnSync(process.execPath, [join(fixture, "Tools/agent/gen-c-abi-routes.mjs"), join(fixture, "runtime.rs"), "--native-list-only"], { encoding: "utf8" });
  success(nestedList);
  assert.ok(JSON.parse(nestedList.stdout).unsupportedGenericCarriers.some((row) => row.type === "JetMeasurement<Vec<f64>>" && row.reason.includes("nested/structural")));
  put("runtime.rs", declarations.replaceAll("JetMeasurement", "__jet_Measurement") + "pub fn jet_fixture_measurement(value: __jet_Measurement<Vec<f64>>) -> __jet_Measurement<f64> { value }\n");
  assert.match(run().stderr, /nested or structural arguments are unsupported/);

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
    streamDefinitions.push([`jet_rt_handle_clone_${runtimeAbiAtom(type)}`, "handle: I64", "I64"], [`jet_rt_handle_drop_${runtimeAbiAtom(type)}`, "handle: I64", ""]);
  }
  streamDefinitions.push(["jet_rt_handle_field_jet_cursor_buf", "handle: I64, output: I64", ""]);
  put("Compiler/JetRuntime/Source/Fixture.jet", "#Export(c) fn jet_fixture_scalar(value: Float) -> Float { value }\n" +
    streamDefinitions.map(([name, params, ret]) => `#Export(c) fn ${name}(${params})${ret ? ` -> ${ret}` : ""} {}`).join("\n"));
  put("Compiler/JetRuntime/exports.list", ["jet_fixture_scalar", ...streamDefinitions.map(([name]) => name)].join("\n") + "\n");
  success(run());
  success(run(true));
  const owned = readFileSync(join(fixture, "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs"), "utf8");
  // The generated Jet fixture above owns these exact definitions:
  // Compiler/JetRuntime/Source/Fixture.jet:31-34 = Reader clone/drop,
  // Cursor clone/drop (29 stream declarations follow the scalar on line 1).
  for (const [name] of streamDefinitions) {
    if (/^jet_rt_handle_(?:clone|drop)_/.test(name)) {
      assert.ok(!owned.includes(`pub extern "C" fn ${name}(`), `${name} owned glue must have no duplicate Rust definition`);
    } else {
      assert.ok(owned.includes(`#[cfg(not(jet_native_pack))]\n    #[no_mangle]\n    pub extern "C" fn ${name}(`), `${name} seed body is not excluded from the native pack`);
    }
  }
  const streamMetadata = readFileSync(join(fixture, "Compiler/JetBackend/Source/Lower/RuntimeRoutes.jet"), "utf8");
  assert.match(streamMetadata, /export: "jet_reader_over", native_lists: true, params: \[LowerRouteCarrier\]\{LowerRouteCarrier.List\{elem: LowerRouteCarrier.Word\}\}/);
  assert.match(streamMetadata, /export: "jet_reader_take", native_lists: true, params: \[LowerRouteCarrier\]\{LowerRouteCarrier.HandleMut\{name: "JetReader"\}, LowerRouteCarrier.Word\}/);
  put("Compiler/JetRuntime/exports.list", ["jet_fixture_scalar", ...streamDefinitions.map(([name]) => name).filter((name) => name !== "jet_rt_handle_clone_jet_reader")].join("\n") + "\n");
  put("Compiler/JetRuntime/Source/Fixture.jet", readFileSync(join(fixture, "Compiler/JetRuntime/Source/Fixture.jet"), "utf8").replace(/^#Export\(c\) fn jet_rt_handle_clone_jet_reader[^\n]*\n/gm, ""));
  const partial = run();
  success(partial);
  const partialStreamRust = readFileSync(join(fixture, "crates/jet-codegen/src/Prelude/Core/CAbiRoutes.rs"), "utf8");
  assert.ok(partialStreamRust.includes('#[cfg(not(jet_native_pack))]\n    #[no_mangle]\n    pub extern "C" fn jet_rt_handle_clone_jet_reader('), "an unported clone must remain seed-only, never acquire a native implementation");
  console.log("CHECK OK — generic schemas, typed stream ABI and generator --check fixtures");
} finally {
  rmSync(fixture, { recursive: true, force: true });
}
