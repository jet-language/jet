import assert from "node:assert/strict";
import test from "node:test";

import {
  buildOracleCatalog,
  batchMutations,
  checkAllAdapters,
  compareTierObservations,
  discoverCorpusSeeds,
  makeResultBundle,
  mutateValueSource,
  readDifferentialManifest,
  regressionFindingBundles,
  serializeBundles,
  validateMutationCase,
} from "../scripts/agent/hardening-oracle-layer.mjs";

const MANIFEST = [
  {
    stable_id: "module:core.math.add",
    kind: "module_call",
    domain: "numeric",
    applicable_tiers: ["aot", "jet_run", "interpreter"],
    seed: "numeric-add-001",
    value_consuming: true,
  },
  {
    stable_id: "receiver:String.lower",
    kind: "receiver_method",
    domain: "text_unicode",
    applicable_tiers: ["aot", "jet_run"],
    seed: "text-lower-001",
    value_consuming: true,
  },
  {
    stable_id: "field:Packet.bytes",
    kind: "field",
    domain: "memory",
    applicable_tiers: ["aot"],
    exclusion: {
      reason: "field is only observable through its owning constructor",
      owner_decision: "D-TEST-FIELD-001",
    },
  },
];

test("catalog derives one independent oracle row per public surface", () => {
  const catalog = buildOracleCatalog(MANIFEST, "sha256:manifest");
  assert.deepEqual(catalog.rows.map((row) => row.stable_id), [
    "field:Packet.bytes",
    "module:core.math.add",
    "receiver:String.lower",
  ]);
  assert.equal(catalog.rows[1].tier_self_diff, true);
  assert.equal(catalog.rows[1].oracle.independence_class, "algebraic-law");
  assert.equal(catalog.exclusions, 1);
  assert.throws(
    () => buildOracleCatalog([MANIFEST[0], MANIFEST[0]], "sha256:manifest"),
    /duplicate surface stable_id/,
  );
  assert.throws(
    () => buildOracleCatalog([{
      stable_id: "module:core.math.add",
      kind: "module_call",
      domain: "numeric",
      applicable_tiers: ["aot"],
      seed: "not-consuming",
      value_consuming: false,
    }], "sha256:manifest"),
    /not value-consuming/,
  );
});

test("mutations preserve typed source shape and observable sink", () => {
  const source = `fn run() {
    value :: 7
    print(value)
}
`;
  const mutated = mutateValueSource(source, {
    domain: "numeric",
    seed: "numeric-add-001",
    mutation_arm: "boundary-max",
  });
  assert.notEqual(mutated.source, source);
  assert.equal(mutated.skeleton, mutateValueSource(mutated.source, {
    domain: "numeric",
    seed: "numeric-add-001",
    mutation_arm: "boundary-min",
  }).skeleton);
  assert.match(mutated.source, /print\(value\)/);
  const fixed = mutateValueSource(`fn run() {
    values :: [Int#3]{1, 2, 3}
    print(values[1])
}
`, { domain: "numeric", seed: "fixed-list-001", mutation_arm: "boundary-max" });
  assert.match(fixed.source, /\[Int#3\]/);
  assert.match(fixed.source, /\{9223372036854775807, 2, 3\}/);
  assert.throws(
    () => validateMutationCase({
      source: "fn run() { result :: uuid.v4()\n print(\"ok\") }",
      domain: "rng_uuid",
      nondeterministic: true,
      normalization: [],
    }),
    /bind-and-discard|normalization/,
  );
  assert.throws(
    () => validateMutationCase({
      source: "fn run() { print(uuid.v4()) }",
      domain: "rng_uuid",
      normalization: [],
    }),
    /normalization/,
  );
  assert.throws(
    () => validateMutationCase({
      source: "fn run() { value :: 1\n print(value) }",
      domain: "numeric",
      skeleton: "different",
    }),
    /skeleton/,
  );
  assert.throws(
    () => validateMutationCase({
      source,
      mutated_source: source.replace("print(value)", "print(\"changed\")"),
    }),
    /bind-and-discard|typed source skeleton/,
  );
});

test("the differential corpus has explicit source/output pairing", () => {
  const rows = readDifferentialManifest();
  assert.equal(rows.length, 65);
  assert.equal(rows.filter((row) => row.output).length, 64);
  assert.deepEqual(rows.filter((row) => !row.output), [{
    source: "ex_basics_loop_values.jet",
    output: null,
    relation: "value-consuming-source",
    exception: "no stable golden; relation-only batch seed",
  }]);
  const discovered = discoverCorpusSeeds(undefined, { includeDifferential: false });
  assert.ok(discovered.seeds.length > 0);
});

test("mutation batches have bounded stable line protocol", () => {
  const batch = batchMutations([{
    stable_surface_id: "module:core.math.add",
    seed: "numeric-add-001",
    domain: "numeric",
    source: "fn run() { value :: 7\n print(value) }\n",
  }], { batchSize: 2 });
  assert.equal(batch.cases.length, 5);
  assert.equal(batch.batches.length, 3);
  assert.equal(batch.batches[0].cases.length, 2);
  assert.equal(batch.cases[0].oracle.independence_class, "algebraic-law");
  assert.equal(batch.cases[0].expected_relation, "oracle:numeric-algebra-laws");
  assert.equal(batch.batches.reduce((text, item) => text + item.line_protocol, "").split("\n").filter(Boolean).length, 5);
  assert.ok(batch.batches.every((item) => item.line_protocol.endsWith("\n")));
});

test("every domain adapter rejects its planted wrong answer", () => {
  const results = checkAllAdapters();
  assert.equal(results.length, 13);
  assert.ok(results.every((result) => result.ok));
});

test("regression seam inversions produce P0 finding bundles", () => {
  const findings = regressionFindingBundles({ commit: "deadbeef" });
  assert.equal(findings.length, 5);
  assert.deepEqual(findings.map((finding) => finding.stable_surface_id), [
    "regression:semantic-equality",
    "regression:indexed-place",
    "regression:packed-int",
    "regression:release-emission",
    "regression:stdin-transport",
  ]);
  assert.ok(findings.every((finding) => finding.classification === "P0"));
  assert.equal(serializeBundles(findings), serializeBundles([...findings].reverse()));
});

test("result bundles are complete and sorted independently of worker order", () => {
  const base = {
    run_id: "run-001",
    stable_surface_id: "module:core.math.add",
    tier: "jet_run",
    tier_command: "scripts/agent/jet-env jet run <batch.jet>",
    seed: "numeric-add-001",
    mutation_arm: "boundary-max",
    source: "fn run() { print(1) }\n",
    expected_relation: "23",
    actual_relation: "23",
    stdout: Buffer.from("23\n"),
    stderr: Buffer.alloc(0),
    exit: 0,
    normalization: [],
    oracle: {
      name: "numeric-law",
      version: "1",
      input_digest: "sha256:input",
      independence_class: "algebraic-law",
      provenance: "test-vector",
    },
    commit: "deadbeef",
    binary_sha256: "sha256:binary",
    registry_snapshot_hash: "sha256:registry",
    config_hash: "sha256:config",
    classification: "pass",
    tower_action: "none",
    tier_observations: [],
  };
  const first = makeResultBundle(base);
  const second = makeResultBundle({ ...base, mutation_arm: "boundary-min" });
  const left = serializeBundles([second, first]);
  const right = serializeBundles([first, second]);
  assert.equal(left, right);
  const decoded = JSON.parse(left.trim().split("\n")[0]);
  for (const field of [
    "schema_version", "run_id", "stable_surface_id", "tier_command", "seed",
    "mutation_arm", "mutator_version", "source", "stdout_bytes", "stderr_bytes",
    "exit", "expected_relation", "actual_relation", "normalization", "oracle",
    "applicable_tiers",
  ]) assert.ok(Object.hasOwn(decoded, field), field);
  assert.deepEqual(compareTierObservations([
    { tier: "aot", stdout_bytes: "base64:eA==", stderr_bytes: "base64:", exit: 0, signal: null, timeout: false, relation: "x" },
    { tier: "jet_run", stdout_bytes: "base64:eA==", stderr_bytes: "base64:", exit: 0, signal: null, timeout: false, relation: "x" },
  ], ["aot", "jet_run"]), { ok: true, baseline: "aot", differences: [] });
  assert.equal(compareTierObservations([
    { tier: "aot", stdout_bytes: "base64:eA==", stderr_bytes: "base64:", exit: 0, signal: null, timeout: false, relation: "x" },
    { tier: "jet_run", stdout_bytes: "base64:eQ==", stderr_bytes: "base64:", exit: 0, signal: null, timeout: false, relation: "y" },
  ], ["aot", "jet_run"]).ok, false);
});
