import assert from "node:assert/strict";
import { nativeHandleExports, partitionNativeHandleExports } from "../../../Tools/agent/native-route-handles.mjs";

// Concrete declaration identities include private fields, payload enums and
// generic instantiations. Layout is deliberately absent: Lower owns it.
const types = Array.from({ length: 10 }, (_, index) => ({
  runtime: `Source${index}`,
  jetName: `<corelib>/core.fixture::Source${index}`,
  jetNames: [`Source${index}`],
  clone: index % 2 === 0,
  eq: index % 3 === 0,
  display: true,
  show: index % 2 === 1,
  debug: index === 0,
  record: { kind: index % 2 ? "enum" : "struct", members: [{ name: "private", kind: "Text" }] },
  source: { path: "Core/fixture/fixture.jet", key: `source-${index}` },
}));
const rustOnly = { ...types[0], runtime: "RustOnly", source: null };
const descriptors = nativeHandleExports([...types, rustOnly]);
assert.equal(descriptors.length, 35);
assert.equal(descriptors.filter(({ op }) => op === "drop").length, 10);
assert.ok(descriptors.every(({ runtime }) => runtime !== "RustOnly"));
assert.deepEqual(descriptors[0], {
  name: "jet_rt_handle_drop_source0", runtime: "Source0", jetName: types[0].jetName, op: "drop",
});
assert.equal(descriptors.filter(({ op }) => op === "clone").length, 5);
assert.equal(descriptors.filter(({ op }) => op === "eq").length, 4);
assert.equal(descriptors.filter(({ op }) => op === "display").length, 10);
assert.equal(descriptors.filter(({ op }) => op === "show").length, 5);
assert.equal(descriptors.filter(({ op }) => op === "debug").length, 1);
const callers = new Map([
  ["jet_rt_handle_clone_source0", ["source:route-a", "source:route-a"]],
  ["jet_rt_handle_eq_source3", ["source:route-b", "source:route-c"]],
  ["jet_rt_handle_drop_rust_only", ["seed:rust-only"]],
]);
const partition = partitionNativeHandleExports([...types, rustOnly], callers);
assert.equal(partition.synthesized.length, 2);
assert.equal(partition.deleted.length, 33);
assert.deepEqual(partition.synthesized[0].callers, ["source:route-a"]);
assert.deepEqual(partition.synthesized[1].callers, ["source:route-b", "source:route-c"]);
assert.ok(partition.deleted.every(({ name }) => !callers.has(name)));
assert.throws(() => nativeHandleExports([types[0], types[0]]), /duplicate native handle export/);
assert.throws(() => nativeHandleExports([{ ...types[0], source: {} }]), /actual source declaration/);
assert.throws(() => partitionNativeHandleExports(types, new Map([["jet_rt_handle_drop_source0", [""]]])), /invalid caller sites/);
assert.throws(() => partitionNativeHandleExports(types, []), /keyed by exact C symbol/);
// Argument lists are already injectively keyed by G1; do not deduce layout
// from a generic name or require private-member projection codecs here.
const generic = { ...types[0], runtime: "Sample__663634", jetName: `${types[0].jetName}<Float>`, record: null };
assert.equal(nativeHandleExports([generic])[0].runtime, "Sample__663634");
console.log("CHECK OK — native handle partition: 10 source types, 35 adapters, 33 deleted, 2 synthesized with caller witnesses; Rust-only unchanged");
