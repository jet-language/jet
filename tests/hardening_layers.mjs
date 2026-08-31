import assert from "node:assert/strict";
import test from "node:test";

import {
  checkPropertyPacks,
  coreRegistrySurfaces,
  generatePropertyCases,
  mapPropertySurfaces,
  propertyPackCatalog,
  propertyLayerSummary,
  runPropertyCases,
  validatePropertyCoverage,
} from "../scripts/agent/hardening-property-layer.mjs";
import {
  classifyGrammarObservation,
  checkGrammarNegativeControls,
  constructManifestHash,
  deriveConstructManifest,
  generateTypedPrograms,
  minimizeGrammarProgram,
  runGrammarPrograms,
  validateConstructManifest,
} from "../scripts/agent/hardening-grammar-layer.mjs";
import { sha256 } from "../scripts/agent/hardening-oracle-layer.mjs";
import {
  MUTATION_CATALOG,
  MUTATION_ADAPTER_CONTRACT,
  applyAstMutation,
  checkMutationAdapterContract,
  checkMutationCatalogShape,
  createMutationAdapter,
  mutationScore,
  runMutationSensitivity,
  validateMustKillCatalog,
  validateMutationCatalog,
} from "../scripts/agent/hardening-mutation-layer.mjs";

const TIERS = ["aot", "jet_run", "interpreter"];

const SURFACES = [
  {
    stable_id: "module:core.math.add",
    domain: "numeric",
    status: "covered",
    applicable_tiers: TIERS,
    value_consuming: true,
  },
  {
    stable_id: "module:core.files.open",
    domain: "host_io",
    status: "excluded",
    exclusion: { reason: "host fixture is owned by the isolation oracle" },
    oracle: "host-isolation-fixture",
  },
];

test("property layer maps the denominator and kills every planted law relation", async () => {
  const coverage = mapPropertySurfaces(SURFACES);
  assert.equal(validatePropertyCoverage(coverage), true);
  assert.equal(coverage.denominator.total, 2);
  assert.equal(coverage.rows.find((row) => row.stable_id === "module:core.math.add").property, true);
  const excluded = coverage.rows.find((row) => row.stable_id === "module:core.files.open");
  assert.match(excluded.reason, /host fixture/);
  assert.ok(excluded.covered_by.includes("host-isolation-fixture"));

  const generated = generatePropertyCases({ surfaces: SURFACES, seed: "test-property", maxCases: 32 });
  const repeated = generatePropertyCases({ surfaces: SURFACES, seed: "test-property", maxCases: 32 });
  assert.deepEqual(generated.cases, repeated.cases);
  assert.ok(generated.cases.every((item) => item.source.includes("fn run") && item.source.includes("print(")));
  const wrong = await runPropertyCases(generated.cases.slice(0, 2), { wrong: true, maxCases: 2 });
  assert.equal(wrong.status, "FINDINGS");
  assert.ok(wrong.findings.every((bundle) => bundle.layer === "property" && bundle.law_id));
  const summary = propertyLayerSummary(generated, wrong);
  assert.equal(summary.valid_case_count, wrong.valid_case_count);
  assert.equal(checkPropertyPacks().every((row) => row.killed), true);

  const realRegistry = generatePropertyCases({ surfaces: coreRegistrySurfaces(), seed: "2341" });
  const packCounts = propertyPackCatalog().map((pack) => ({
    pack: pack.id,
    valid_case_count: realRegistry.cases.filter((item) => item.pack === pack.id).length,
  }));
  assert.ok(packCounts.every((row) => row.valid_case_count > 0), JSON.stringify(packCounts));
});

test("grammar layer derives construct/production denominator and records stage mismatches", async () => {
  const manifest = deriveConstructManifest({
    syntaxSource: '/// ratified D-GRAMMAR-TEST\npub const KW_IF: &str = "if";',
    parserSources: ["pub fn parse_expr() {}"],
    semaSources: ["pub fn infer_expr() {}"],
  });
  assert.equal(validateConstructManifest(manifest), true);
  for (const family of ["expressions", "statements", "control-flow", "patterns", "generics", "traits", "effects", "views", "comptime", "nested-places"]) {
    assert.ok(manifest.rows.some((row) => row.construct_id === `family:${family}`));
  }
  assert.ok(manifest.denominator.total >= 12);
  const generated = generateTypedPrograms(manifest, { seed: "test-grammar", maxCases: 24, includeNearValid: true });
  const repeated = generateTypedPrograms(manifest, { seed: "test-grammar", maxCases: 24, includeNearValid: true });
  assert.deepEqual(generated.programs, repeated.programs);
  assert.equal(generated.manifest_sha256, constructManifestHash(manifest));
  assert.equal(generated.programs_sha256, repeated.programs_sha256);
  assert.ok(generated.programs.some((program) => !program.value_consuming));
  assert.ok(generated.programs.filter((program) => program.value_consuming).every((program) => program.observable_sink?.type_aware));
  assert.ok(generated.programs.filter((program) => !program.value_consuming).every((program) => {
    return typeof program.violated_property === "string" && !program.expected_diagnostic;
  }));
  const first = generated.programs.find((program) => program.value_consuming);
  const minimized = minimizeGrammarProgram(first, [first.source + "unused\n"]);
  assert.equal(minimized.construct_id, first.construct_id);
  assert.equal(minimized.source.includes("print("), true);

  const result = await runGrammarPrograms([first], {
    executor: ({ tier }) => ({ tier, normalized_value: tier === "aot" ? 1 : 2, exit: 0, signal: null, timeout: false }),
    maxCases: 1,
  });
  assert.equal(result.status, "FINDINGS");
  assert.equal(result.findings[0].layer, "grammar");
  const stages = classifyGrammarObservation({
    parser: { accepted: true },
    sema: { accepted: true },
    tir: { constructed: false, evaluated: false },
    rust: { accepted: false },
    tiers: Object.fromEntries(TIERS.map((tier) => [tier, { normalized_value: 1, exit: 0 }])),
    program: first,
  });
  assert.equal(stages.status, "RED");
  assert.equal(stages.classification, "internal-I2");
  const rustRejection = classifyGrammarObservation({
    parser: { accepted: true },
    sema: { accepted: true },
    tir: { constructed: true, evaluated: true },
    tiers: Object.fromEntries(TIERS.map((tier) => [tier, {
      normalized_value: 1,
      exit: tier === "aot" ? 1 : 0,
      error: tier === "aot" ? "rustc rejected generated Rust" : undefined,
    }])),
    rust: { accepted: true },
    program: first,
  });
  assert.equal(rustRejection.classification, "internal-I2");
  assert.deepEqual(checkGrammarNegativeControls(), [
    "missing-production",
    "observerless",
    "admitted-but-unlowered",
    "optimizer-only-meaning-change",
  ]);
});

test("grammar layer records real near-valid diagnostics and observes the full corpus", async () => {
  const manifest = deriveConstructManifest({
    syntaxSource: '/// ratified D-GRAMMAR-COVERAGE\npub const KW_IF: &str = "if";',
    parserSources: ["pub fn parse_expr() {}"],
    semaSources: ["pub fn infer_expr() {}"],
  });
  const generated = generateTypedPrograms(manifest, { seed: "2342", maxCases: 128, includeNearValid: true });
  const run = await runGrammarPrograms(generated.programs, {
    maxCases: 128,
    stageExecutor: async (program) => program.value_consuming
      ? { parser: { accepted: true }, sema: { accepted: true } }
      : { parser: { accepted: false, diagnostics: [{ code: "E0003", span: { start: 101, end: 101 } }] } },
    executor: async ({ tier, value_consuming }) => value_consuming
      ? { tier, normalized_value: 1, tir: { constructed: true, evaluated: true }, exit: 0 }
      : { tier, stderr: "error[E0003]", exit: 1 },
  });
  assert.equal(run.status, "PASS");
  assert.equal(run.attempted, 128);
  assert.equal(run.valid_case_count, 64);
  assert.equal(run.near_valid_case_count, 64);
  assert.equal(run.program_results.length, 128);
  assert.equal(run.coverage.expected_programs, 128);
  assert.equal(run.coverage.observed_programs, 128);
  assert.equal(run.coverage.expected_valid_programs, 64);
  assert.equal(run.coverage.observed_valid_programs, 64);
  assert.equal(run.coverage.complete, true);
  assert.deepEqual(run.coverage.unobserved_cells, []);
  for (const counts of Object.values(run.coverage.stages)) assert.equal(counts.observed, counts.applicable);
  for (const counts of Object.values(run.coverage.tiers)) assert.equal(counts.observed, counts.applicable);
  for (const result of run.program_results) {
    assert.ok(result.stages.parser.observed);
    for (const tier of result.applicable_tiers) assert.ok(result.tiers[tier].observed);
    for (const stage of Object.values(result.stages)) {
      if (stage.applicable) assert.ok(stage.observed);
    }
  }
  const near = run.program_results.find((program) => !program.value_consuming);
  assert.deepEqual(near.observed_diagnostic, {
    code: "E0003",
    stage: "parser",
    registered: true,
    observed: true,
  });
  assert.equal(near.classification, "registered-diagnostic");

  const unregistered = await runGrammarPrograms([generated.programs.find((program) => !program.value_consuming)], {
    stageExecutor: async () => ({ parser: { accepted: false, diagnostics: [{ code: "E9999" }] } }),
    executor: async ({ tier }) => ({ tier, stderr: "error[E9999]", exit: 1 }),
  });
  assert.equal(unregistered.status, "FINDINGS");
  assert.match(unregistered.findings[0].proof.observable_mismatch.errors.join("; "), /unregistered/);

  const staleExpectation = {
    ...generated.programs.find((program) => !program.value_consuming),
    expected_diagnostic: { code: "E0306" },
  };
  const stale = await runGrammarPrograms([staleExpectation], {
    stageExecutor: async () => ({ parser: { accepted: false, diagnostics: [{ code: "E0003" }] } }),
    executor: async ({ tier }) => ({ tier, stderr: "error[E0003]", exit: 1 }),
  });
  assert.equal(stale.status, "FINDINGS");
  assert.match(stale.findings[0].proof.observable_mismatch.errors.join("; "), /expected E0306/);
});

test("mutation layer applies AST descriptors serially and turns disabled killers into deduplicated gaps", async () => {
  validateMutationCatalog(MUTATION_CATALOG);
  assert.equal(validateMustKillCatalog(MUTATION_CATALOG), true);
  assert.equal(checkMutationCatalogShape().length, MUTATION_CATALOG.length);
  const ast = {
    functions: {
      values_equal: {
        list_arm: {
          expression: { kind: "expression", source: "list_values_equal(left, right)" },
        },
      },
    },
  };
  assert.equal(
    applyAstMutation(ast, MUTATION_CATALOG[0]).functions.values_equal.list_arm.expression.source,
    "left.len() == right.len()",
  );

  const catalog = MUTATION_CATALOG.slice(0, 2);
  const baseline = { source_sha256: sha256(catalog[0].witness.source), target_sha256: "sha256:target" };
  let active = 0;
  let maximum = 0;
  const summary = await runMutationSensitivity({
    catalog,
    baseline,
    disabledKillers: [catalog[1].id],
    apply: async (mutant) => {
      active += 1;
      maximum = Math.max(maximum, active);
      const source = `${mutant.witness.source}// mutated ${mutant.id}\n`;
      active -= 1;
      return { source };
    },
    build: async () => ({ ok: true }),
    prove: async (layer) => ({ layer, status: "killed", killed: true, value_consuming: true, observable_mismatch: true, tier: "jet_run" }),
    restore: async () => {},
    baseline: { ...baseline, current: async () => baseline },
  });
  assert.equal(maximum, 1);
  assert.equal(summary.attempted, 2);
  assert.equal(summary.killed, 1);
  assert.equal(summary.survivors, 1);
  assert.equal(summary.gap_cards.length, 1);
  assert.equal(summary.results[1].proof.skipped, true);
  assert.equal(Object.isFrozen(summary.bundles), true);
  assert.equal(summary.status, "RED");
  assert.equal(mutationScore(summary).score, 0.5);
});

test("mutation layer rejects non-observations and exposes omitted catalog entries", async () => {
  const catalog = MUTATION_CATALOG.slice(0, 2);
  const baselineSource = catalog[0].witness.source;
  const targetSha256 = `sha256:${"b".repeat(64)}`;
  const baseline = {
    source_sha256: sha256(baselineSource),
    target_sha256: targetSha256,
    commit: "mutation-test-commit",
    source: baselineSource,
    current: async () => ({
      source_sha256: sha256(baselineSource),
      target_sha256: targetSha256,
      commit: "mutation-test-commit",
    }),
  };
  let removed = 0;
  const failed = await runMutationSensitivity({
    catalog,
    maxMutants: 1,
    baseline,
    workspaceRequired: true,
    removeWorkspace: async () => { removed += 1; },
    apply: async (_mutant, input) => ({ source: `${input.source}\n// mutation\n` }),
    build: async () => ({ ok: true }),
    prove: async (layer) => ({
      layer,
      status: "killed",
      killed: true,
      value_consuming: true,
      observable_mismatch: true,
      exit: 7,
    }),
    restore: async () => {},
    metadata: { commit: baseline.commit, binary_sha256: targetSha256 },
  });
  assert.equal(failed.killed, 0);
  assert.equal(failed.survivors, 2);
  assert.equal(failed.status, "RED");
  assert.deepEqual(failed.omitted_mutant_ids, [catalog[1].id]);
  assert.equal(removed, 1);
  assert.equal(failed.gap_cards.length, 2);
  assert.ok(failed.gap_cards.some((card) => card.payload.missing_proof.status === "not-attempted"));
  assert.equal(failed.results[0].proof.baseline_restored, true);
  assert.equal(failed.results[0].proof.workspace_removed, true);
});

test("mutation layer requires an executable baseline checksum probe for worktree runs", async () => {
  const catalog = MUTATION_CATALOG.slice(0, 1);
  const baseline = {
    source_sha256: sha256(catalog[0].witness.source),
    target_sha256: `sha256:${"c".repeat(64)}`,
  };
  await assert.rejects(
    runMutationSensitivity({
      catalog,
      baseline,
      workspaceRequired: true,
      removeWorkspace: async () => {},
      apply: async (_mutant, input) => ({ source: `${input.source}\n// mutation\n` }),
      build: async () => ({ ok: true }),
      prove: async () => ({ layer: "property", killed: true, value_consuming: true, observable_mismatch: true }),
      restore: async () => {},
    }),
    /baseline\.current checksum callback/,
  );
});

test("mutation adapter contract is real and non-value observations cannot kill", async () => {
  const adapter = createMutationAdapter({ root: process.cwd() });
  assert.equal(checkMutationAdapterContract(adapter), true);
  assert.ok(MUTATION_ADAPTER_CONTRACT.guarantees.includes("disposable-worktree"));
  assert.equal(typeof adapter.baseline_source, "string");

  const mutant = MUTATION_CATALOG[0];
  const baseline = { source_sha256: sha256(mutant.witness.source), target_sha256: "sha256:target" };
  const summary = await runMutationSensitivity({
    catalog: [mutant],
    baseline,
    apply: async (_entry, input) => ({ source: `${input.source}\n// mutation\n` }),
    build: async () => ({ ok: true }),
    prove: async (layer) => ({
      layer,
      status: "killed",
      killed: true,
      value_consuming: false,
      observable_mismatch: true,
      exit: 0,
    }),
    restore: async () => {},
  });
  assert.equal(summary.killed, 0);
  assert.equal(summary.results[0].proof.value_consuming, false);
});
