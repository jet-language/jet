// The gauntlet driver of Tools/gauntlet/harness/performance-surface.mjs
// (#5072 c8): the paired surface/plain measurement, its receipt's validation
// and its command line. Tools/gauntlet/entries/run-performance-surface/driver/run.jet
// is the twin on the kit's PerformanceSurface module.
//
//   node run-performance-surface.mjs measure DIR
//   node run-performance-surface.mjs manifest
//   node run-performance-surface.mjs main [ARG...]
//
// `measure` measures each scenario of DIR (JSON files, in name order) with
// stand-in builds and runs: the compiler `identity`, the manifest `pair` row
// measured (none for the compound pair), the `manifest` the measurement is
// held to, each arm's build fields (`builds`), its wall times by sample
// index (`times`, cycled) and the fields of a named sample that went wrong
// (`faults`). Each is shown as its name, the receipt's status, verdict and
// preflight issues, the receipt's SHA-256 (artifact_md5 left out: the twin
// identifies the artifact by SHA-256 alone) and its validation: the verdict,
// the count, first three and SHA-256 of its findings, and the SHA-256 of its
// statistics; a scenario with a `patch` (JSON Patch operations) shows the
// validation of the receipt so changed, against `spec` when it names one.
// `manifest` shows each pair of the canonical measurement manifest,
// Tools/gauntlet/measurement-manifest.json, with the manifest's own issues and
// the validation of the evidence receipt it names. `main` runs the command
// line, an error shown on stdout.
import path from 'node:path';
import fs from 'node:fs';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { PAIR_ID, main, measurePair, validateManifestPair, validatePerformanceSurfaceReceipt } from '../performance-surface.mjs';

const usage = 'usage: node run-performance-surface.mjs measure DIR | manifest | main [ARG...]';
const [command, ...rest] = process.argv.slice(2);
const repo = fileURLToPath(new URL('../../../../', import.meta.url));
const sha256 = (text) => createHash('sha256').update(text).digest('hex');
const readJson = (file) => JSON.parse(fs.readFileSync(file, 'utf8'));

// A validation as its verdict, the count, first three and SHA-256 of its
// findings, and the SHA-256 of its statistics (its pair row is the
// receipt's own).
function shown(validation) {
  const { pair, statistics, findings, ...kept } = validation;
  return `${JSON.stringify(kept)}\t${findings.length} findings ${JSON.stringify(findings.slice(0, 3))} ${sha256(JSON.stringify(findings))}\t${sha256(JSON.stringify(statistics ?? null))}`;
}

// A sample of `request` as the scenario has it: the oracle's output in its
// cycled wall time unless a fault says otherwise.
function sample({ label, source, expectedOutput, context }, scenario) {
  const [, index, arm] = label.split('-');
  const times = scenario.times?.[arm] ?? [arm === 'surface' ? 10 : 20];
  const fault = scenario.faults?.[label] ?? {};
  const stdout = fault.stdout ?? expectedOutput;
  const matches = stdout === expectedOutput;
  return {
    label,
    source_file: 'main.jet',
    source_sha256: sha256(source),
    pair_context_sha256: context.context_sha256,
    artifact_sha256: context.artifact_sha256,
    environment_sha256: context.environment_sha256,
    tier: context.tier,
    target: context.target,
    input_range: context.inputs.range,
    output_oracle: context.output_validation.oracle,
    wall_time_ms: times[Number(index) % times.length],
    status: 0,
    exit_code: 0,
    signal: null,
    timed_out: false,
    error_code: null,
    stdout,
    stderr: '',
    stdout_bytes_base64: Buffer.from(stdout, 'utf8').toString('base64'),
    stderr_bytes_base64: '',
    output_verified: matches,
    output_matches: matches,
    contaminated: false,
    directory_entries: [],
    ...fault,
  };
}

// `pointer`'s steps, unescaped.
const steps = (pointer) => pointer.split('/').slice(1).map((step) => step.replaceAll('~1', '/').replaceAll('~0', '~'));

// `operations` (add, remove and replace) applied to `tree` in place.
function patch(tree, operations) {
  for (const { op, path: pointer, value } of operations) {
    const parts = steps(pointer);
    const last = parts.pop();
    const parent = parts.reduce((node, step) => node[Array.isArray(node) ? Number(step) : step], tree);
    if (Array.isArray(parent)) {
      const index = last === '-' ? parent.length : Number(last);
      if (op === 'add') parent.splice(index, 0, value);
      else if (op === 'remove') parent.splice(index, 1);
      else parent[index] = value;
    } else if (op === 'remove') {
      delete parent[last];
    } else {
      parent[last] = value;
    }
  }
  return tree;
}

function measure(dir) {
  const names = fs.readdirSync(dir).filter((name) => name.endsWith('.json')).sort();
  for (const name of names) {
    const scenario = readJson(path.join(dir, name));
    const result = measurePair({
      repoDir: process.cwd(),
      artifactPath: path.resolve(scenario.artifact ?? 'fake-jet.txt'),
      scratchRoot: 'scratch',
      identity: scenario.identity,
      pair: scenario.pair ?? null,
      manifest: scenario.manifest ?? null,
      generatedAt: '2026-01-01T00:00:00.000Z',
      generatedCommand: 'fixture',
      buildLine: 'fixture',
      buildArm: ({ source, label }) => ({
        root: null,
        executable: `${label}-main`,
        executable_sha256: 'b'.repeat(64),
        source_sha256: sha256(source),
        status: 0,
        signal: null,
        error_code: null,
        stderr: '',
        ...(scenario.builds?.[label] ?? {}),
      }),
      runArm: (request) => sample(request, scenario),
    });
    const { receipt } = result;
    const { artifact_md5, ...kept } = receipt;
    const digest = sha256(JSON.stringify(kept));
    const head = `${name}\t${kept.status} ${kept.verdict}\t${JSON.stringify(kept.preflight_issues)}\t${digest}`;
    // The receipt patched is the receipt as written, so no two of its
    // members share one value.
    const validation = scenario.patch
      ? validatePerformanceSurfaceReceipt(patch(JSON.parse(JSON.stringify(receipt)), scenario.patch), { pairSpec: 'spec' in scenario ? scenario.spec : scenario.pair ?? null })
      : result.validation;
    console.log(`${head}\t${shown(validation)}`);
  }
}

function manifest() {
  const surface = readJson(path.join(repo, 'Tools/gauntlet/measurement-manifest.json')).performance_surface_pairs;
  for (const spec of surface.pairs) {
    const issues = spec.id === PAIR_ID ? validateManifestPair(spec, surface.evidence, surface.pair_measurement) : [];
    const file = spec.evidence_file ?? 'Tools/gauntlet/performance-surface-evidence.json';
    const validation = validatePerformanceSurfaceReceipt(readJson(path.join(repo, file)), { pairSpec: spec });
    console.log(`${spec.id}\t${file}\t${JSON.stringify(issues)}\t${shown(validation)}`);
  }
}

if (command === 'measure' && rest.length > 0) {
  measure(rest[0]);
} else if (command === 'manifest') {
  manifest();
} else if (command === 'main') {
  try {
    process.exitCode = await main(rest);
  } catch (error) {
    console.log(`performance surface: ${error.message}`);
    process.exitCode = 1;
  }
} else {
  console.error(usage);
  process.exit(64);
}
