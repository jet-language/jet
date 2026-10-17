import assert from "node:assert/strict";
import test from "node:test";
import { excludeNativeSeedBodies, nativeSeedOnlyExports, sourceOwnedAdapterProofs } from "./native-route-partition.mjs";
const seed = (name) => `    #[no_mangle]\n    pub extern "C" fn ${name}(value: *mut crate::SharedCarrier) -> i64 {\n        1\n    }\n`;
test("shared carrier members are partitioned per symbol without a Rust fallback", () => {
  const providers = new Set(["native_member"]);
  const adapters = [{name: "native_member"}, {name: "unavailable_member"}];
  const only = nativeSeedOnlyExports(new Set(["drop_shared"]), adapters, providers);
  assert.deepEqual([...only].sort(), ["drop_shared", "unavailable_member"]);
  const validated = [];
  const output = excludeNativeSeedBodies(seed("native_member") + seed("unavailable_member") + seed("drop_shared"), providers, only, (name) => validated.push(name));
  assert.deepEqual(validated, ["native_member"]);
  assert.equal([...output.matchAll(/#\[cfg\(not\(jet_native_pack\)\)\]/g)].length, 3);
  assert.equal([...output.matchAll(/pub extern "C" fn/g)].length, 3);
  assert.throws(() => excludeNativeSeedBodies(seed("unregistered"), providers, only, () => {}), /unpartitioned Rust native-pack export/);
});
test("external C imports are not seed adapter definitions", () => {
  const only = nativeSeedOnlyExports([], [{name: "libc_leaf", external: true}], new Set());
  assert.equal(only.size, 0);
});
const ledger = 'CoreSourceModule { module: "core.example", alias: "example", path: "Core/example/example.jet", owned_members: &["parse"] }\nCoreSourceOwnedType { path: "Core/example/example.jet", name: "Value" }';
const source = 'fn parse(text: String) -> Value { Value{} }\nstruct Value { fn inspect(self) -> Int { 1 } }';
test("function source proof requires every registry label and a current body", () => {
  const adapter = {name: "parse", symbol: "legacy_parse", rows: ["core.example.parse"], params: []};
  assert.equal(sourceOwnedAdapterProofs([adapter], ledger, "", () => source).get("parse").kind, "source-function");
  assert.equal(sourceOwnedAdapterProofs([{...adapter, rows: [...adapter.rows, "core.example.__parse_raw"]}], ledger, "", () => source).size, 0);
  assert.equal(sourceOwnedAdapterProofs([adapter], ledger, "", () => "").size, 0);
  assert.equal(sourceOwnedAdapterProofs([adapter], ledger, "", () => "// fn parse(text: String) { }\n").size, 0);
  assert.equal(sourceOwnedAdapterProofs([adapter], ledger, "", () => "fn parse(text: String)\nfn other() { }\n").size, 0);
});
test("method proof requires the exact TIR arm and an owned declaration", () => {
  const adapter = {name: "legacy_inspect", symbol: "LegacyValue::inspect", rows: ["TIR/routes.rs"], params: []};
  const tir = '("Value", "inspect") => ("LegacyValue::inspect", 1, &[true])';
  const proof = sourceOwnedAdapterProofs([adapter], ledger, tir, () => source).get(adapter.name);
  assert.deepEqual(proof, {kind: "source-method", declarations: [{path: "Core/example/example.jet", type: "Value", member: "inspect"}]});
  assert.equal(sourceOwnedAdapterProofs([adapter], ledger, tir.replace("LegacyValue::inspect", "Other::inspect"), () => source).size, 0);
  assert.equal(sourceOwnedAdapterProofs([adapter], ledger.replace('name: "Value"', 'name: "Other"'), tir, () => source).size, 0);
});
