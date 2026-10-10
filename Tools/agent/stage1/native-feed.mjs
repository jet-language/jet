#!/usr/bin/env node
// Sharded bootstrap factory -> real native CLI stage1 -> real argv stage2.
// The factory's Rust host is a bootstrap exception, never a native compiler.
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { accessSync, chmodSync, closeSync, constants, copyFileSync, cpSync, existsSync, mkdirSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { invokeStageZeroFactory, readStageZeroFactory } from './native-factory.mjs';
import { readNativeRuntimePack } from '../../packaging/runtime-pack-cache.mjs';
const producerRepo = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const options = { repo: producerRepo };
const usage = 'native-feed.mjs --factory-record FILE --runtime-dir DIR --out FRESH_DIR [--repo DIR] [--dry-run]';
for (let i = 2; i < process.argv.length; i++) {
  const flag = process.argv[i];
  if (flag === '--dry-run') { options.dryRun = true; continue; }
  const key = { '--factory-record': 'factory', '--runtime-dir': 'runtime', '--out': 'out', '--repo': 'repo' }[flag];
  if (!key || !process.argv[i + 1] || process.argv[i + 1].startsWith('--')) throw Error(usage);
  options[key] = resolve(process.argv[++i]);
}
if (!options.factory || !options.runtime || !options.out) throw Error(usage);
if (existsSync(options.out)) throw Error(`fresh native-feed output required: ${options.out}`);
mkdirSync(options.out, { recursive: true });
const { repo, out } = options;
const json = (file, value) => writeFileSync(file, JSON.stringify(value, null, 2) + '\n');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const steps = [];
function execute(name, command, env, cwd) {
  const stdout = join(out, `${name}.stdout`), stderr = join(out, `${name}.stderr`);
  const start = Date.now();
  const a = openSync(stdout, 'w'), b = openSync(stderr, 'w');
  let result;
  try { result = spawnSync(command[0], command.slice(1), { env, cwd, stdio: ['ignore', a, b] }); }
  finally { closeSync(a); closeSync(b); }
  const step = { name, command, rc: result.status ?? 1, signal: result.signal, error: result.error?.message, seconds: (Date.now() - start) / 1000, stdout, stderr };
  steps.push(step); json(join(out, 'steps.json'), steps);
  if (step.rc) throw Error(`${name} failed; see ${stderr}`);
}
const assembly = join(out, 'assembly');
execute('assemble', [process.execPath, join(producerRepo, 'Tools/agent/native-compiler.mjs'), '--assemble-only', '--repo', repo, '--out', assembly], process.env, repo);
const unit = JSON.parse(readFileSync(join(assembly, 'plan.json'), 'utf8'));
const stage1 = join(out, 'stage1'), stage2 = join(out, 'stage2');
const prefix = join(stage1, 'factory-output');
const plan = { schema: 'jet-native-feed-plan/v1', sourceRoot: repo, source: unit.source, sourceSha256: unit.sourceSha256, map: unit.map, entry: unit.entry, factoryRecord: options.factory, runtimeDir: options.runtime, producerSha256: hash(readFileSync(fileURLToPath(import.meta.url))), harnessSha256: hash(readFileSync(new URL('./native-factory.mjs', import.meta.url))), stages: ['source-bound AOT factory -> native stage1', 'native stage1 real argv -> native stage2'], comparisons: ['stage1/jet.o == stage2/jet.o', 'stage1/jet == stage2/jet'], allPhasesReportsMandatory: true, optimization: 'O0' };
json(join(out, 'plan.json'), plan);
if (options.dryRun) { console.log(JSON.stringify({ ...plan, dryRun: true, upstreamArtifactsNotYetAdmitted: true }, null, 2)); process.exit(0); }
let receipt;
try {
  const record = readStageZeroFactory(options.factory, repo);
  const runtime = readNativeRuntimePack(options.runtime);
  if (runtime.provenance.bootstrapFactory?.sourceCommit !== record.commit || runtime.provenance.compilerSha256 !== record.factory.sha256) throw Error('runtime pack must be produced by this same source-bound factory');
  const packPath = join(options.runtime, runtime.key, 'jet_runtime.pack.o');
  mkdirSync(stage1);
  const start = Date.now();
  const factory = invokeStageZeroFactory({ recordFile: options.factory, repo, sourceRoot: unit.project, entry: 'selfhost.jet', work: join(out, 'factory-native'), policy: { JET_FRONT_NATIVE_OUTPUT: prefix, JET_RUNTIME_PACK: packPath } });
  for (const suffix of ['.issues', '.pack-issues']) {
    const file = prefix + suffix;
    if (existsSync(file)) { const issues = JSON.parse(readFileSync(file, 'utf8')); if (!Array.isArray(issues) || issues.length) throw Error(`native stage1 codegen/link failures: ${file}`); }
  }
  function publish(directory, binary, object) {
    accessSync(binary, constants.X_OK);
    const bytes = readFileSync(binary), objects = readFileSync(object);
    for (const value of [bytes, objects]) if (value.subarray(0, 4).toString('hex') !== '7f454c46') throw Error('native feed did not produce both ELF object and executable');
    mkdirSync(directory, { recursive: true });
    copyFileSync(binary, join(directory, 'jet')); chmodSync(join(directory, 'jet'), 0o755);
    copyFileSync(object, join(directory, 'jet.o'));
    cpSync(options.runtime, join(directory, 'jet-runtime'), { recursive: true, dereference: true });
    return { jet: join(directory, 'jet'), sha256: hash(bytes), objectSha256: hash(objects) };
  }
  const first = publish(stage1, prefix + '.exe', prefix + '.o');
  steps.push({ name: 'factory-native-stage1', rc: 0, seconds: (Date.now() - start) / 1000, factoryReceipt: factory.receipt, keepGoingReport: factory.census, ...first });
  json(join(out, 'steps.json'), steps);
  const env = { ...process.env, JET_TOOLCHAIN_ROOT: repo, JET_KEEP_GOING: '1', JET_KEEP_GOING_REPORT: join(out, 'stage2-keep-going.tsv') };
  for (const key of Object.keys(env)) if (key.startsWith('JET_FRONT_') || key.startsWith('JET_BOOTSTRAP_') || key === 'JET_DUMP_MIR' || key === 'JET_DEBUG_LOWER_ALL' || key === 'JET_KEEP_GOING_SKIP') delete env[key];
  execute('native-stage2', [join(repo, 'Tools/agent/jet-env'), first.jet, 'build', unit.source], env, unit.project);
  const census = readFileSync(env.JET_KEEP_GOING_REPORT, 'utf8').trim().split('\n');
  if (census.length !== 1 || census[0] !== 'phase\titem\tcode\tmessage\tspan') throw Error('stage2 all-phases census must exist and be empty');
  const second = publish(stage2, join(unit.project, '.jet/build/selfhost'), join(unit.project, '.jet/build/selfhost.o'));
  const comparisons = ['jet', 'jet.o'].map(name => ({ name, equal: readFileSync(join(stage1, name)).equals(readFileSync(join(stage2, name))) }));
  json(join(out, 'comparisons.json'), comparisons);
  if (comparisons.some(row => !row.equal)) throw Error('native stage1/stage2 object and ELF fixed point differ');
  receipt = { schema: 'jet-native-compiler/v1', commit: record.commit, sourceRoot: repo, sourceSha256: unit.sourceSha256, factorySha256: record.factory.sha256, runtimePackSha256: hash(runtime.pack), backend: 'jet-native', optimization: 'O0', fixedPoint: true, stages: [{ stage: 1, ...first, keepGoingReport: factory.census }, { stage: 2, ...second, keepGoingReport: env.JET_KEEP_GOING_REPORT }], jet: second.jet, runtimeDir: join(stage2, 'jet-runtime'), steps, rc: 0, verdict: 'GREEN' };
} catch (error) {
  receipt = { schema: 'jet-native-feed-failure/v1', plan, steps, rc: 1, verdict: 'RED', error: error.stack, fixedPoint: false };
}
json(join(out, 'native-compiler.json'), receipt);
writeFileSync(join(out, 'driver.rc'), String(receipt.rc) + '\n');
console.log(JSON.stringify(receipt, null, 2)); process.exitCode = receipt.rc;
