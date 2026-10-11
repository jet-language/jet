#!/usr/bin/env node
// Sharded bootstrap factory -> real native CLI stage1 -> real argv stage2
// (gate: stage1 == stage2, whole files) -> real argv stage3 (receipt:
// stage2 == stage3, the native compiler rebuilding itself).
// The factory's Rust host is a bootstrap exception, never a native compiler.
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { accessSync, chmodSync, closeSync, constants, copyFileSync, cpSync, existsSync, mkdirSync, openSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
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
const stage1 = join(out, 'stage1'), stage3 = join(out, 'stage3');
const prefix = join(stage1, 'factory-output');
const plan = { schema: 'jet-native-feed-plan/v1', sourceRoot: repo, source: unit.source, sourceSha256: unit.sourceSha256, map: unit.map, entry: unit.entry, factoryRecord: options.factory, runtimeDir: options.runtime, producerSha256: hash(readFileSync(fileURLToPath(import.meta.url))), harnessSha256: hash(readFileSync(new URL('./native-factory.mjs', import.meta.url))), stages: ['source-bound AOT factory -> native stage1', 'native stage1 real argv -> native stage2', 'native stage2 real argv -> native stage3'], comparisons: ['stage1/jet.o == stage2/jet.o', 'stage1/jet == stage2/jet'], selfRebuild: ['stage2/jet.o == stage3/jet.o', 'stage2/jet == stage3/jet'], stack: '1 GiB RLIMIT_STACK (prlimit) for every native CLI stage', allPhasesReportsMandatory: true, optimization: 'O0' };
json(join(out, 'plan.json'), plan);
if (options.dryRun) { console.log(JSON.stringify({ ...plan, dryRun: true, upstreamArtifactsNotYetAdmitted: true }, null, 2)); process.exit(0); }
let receipt;
try {
  const record = readStageZeroFactory(options.factory, repo);
  // The native CLI takes its compiler identity only from the environment
  // (JetCli Request.jet jet_cli_driver_request) and refuses to compile without
  // it (Pipeline.jet E0003). Every native stage must use the identity the
  // factory baked in (Bootstrap/Host/Native.rs, `jet@<version>#<source id>`).
  const identity = record.factory.compiler_identity;
  if (typeof identity !== 'string' || !/^[^@#\s]+@[^@#\s]+#[^@#\s]+$/.test(identity)) throw Error('factory receipt lacks factory.compiler_identity (`name@version#source-id`); refusing to invent one');
  const runtime = readNativeRuntimePack(options.runtime);
  if (runtime.provenance.bootstrapFactory?.sourceCommit !== record.commit || runtime.provenance.compilerSha256 !== record.factory.sha256) throw Error('runtime pack must be produced by this same source-bound factory');
  const packPath = join(options.runtime, runtime.key, 'jet_runtime.pack.o');
  mkdirSync(stage1);
  const start = Date.now();
  const factory = invokeStageZeroFactory({ recordFile: options.factory, repo, sourceRoot: unit.project, entry: 'selfhost.jet', work: join(out, 'factory-native'), policy: { JET_FRONT_NATIVE_OUTPUT: prefix, JET_RUNTIME_PACK: packPath } });
  for (const suffix of ['.issues', '.pack-issues']) {
    const file = prefix + suffix;
    if (existsSync(file) && readFileSync(file, 'utf8').trim()) throw Error(`native stage1 codegen/link findings: ${file}`);
  }
  function publish(directory, binary, object) {
    accessSync(binary, constants.X_OK);
    const bytes = readFileSync(binary), objects = readFileSync(object);
    for (const value of [bytes, objects]) if (value.subarray(0, 4).toString('hex') !== '7f454c46') throw Error('native feed did not produce both ELF object and executable');
    mkdirSync(directory, { recursive: true });
    copyFileSync(binary, join(directory, 'jet')); chmodSync(join(directory, 'jet'), 0o755);
    copyFileSync(object, join(directory, 'jet.o'));
    cpSync(options.runtime, join(directory, 'jet-runtime'), { recursive: true, dereference: true });
    return { binary: join(directory, 'jet'), sha256: hash(bytes), objectSha256: hash(objects) };
  }
  const first = publish(stage1, prefix + '.exe', prefix + '.o');
  steps.push({ name: 'factory-native-stage1', rc: 0, seconds: (Date.now() - start) / 1000, factoryReceipt: factory.receipt, keepGoingReport: factory.census, ...first });
  json(join(out, 'steps.json'), steps);
  const artifactsFile = join(dirname(repo), 'ARTIFACTS.json');
  const artifacts = existsSync(artifactsFile) ? JSON.parse(readFileSync(artifactsFile, 'utf8')) : { schema: 'jet-regression-artifacts/v1', regression_role: 'discovery', commit: record.commit, tree: record.tree, source: repo, target_dir: `/mnt/jetscratch/targets/source-${record.commit}`, artifacts: {} };
  if (artifacts.schema !== 'jet-regression-artifacts/v1' || artifacts.commit !== record.commit || artifacts.tree !== record.tree || artifacts.source !== repo) throw Error('artifact publication source pin mismatch');
  artifacts.artifacts.candidate_jetc0 = { path: first.binary, sha256: first.sha256, native: true, runtimeDir: join(stage1, 'jet-runtime'), sourceSha256: unit.sourceSha256 };
  const temporaryArtifacts = `${artifactsFile}.${process.pid}.new`;
  json(temporaryArtifacts, artifacts); renameSync(temporaryArtifacts, artifactsFile);
  const env = { ...process.env, JET_TOOLCHAIN_ROOT: repo, JET_KEEP_GOING: '1', JET_COMPILER_IDENTITY: identity };
  for (const key of Object.keys(env)) if (key.startsWith('JET_FRONT_') || key.startsWith('JET_BOOTSTRAP_') || key === 'JET_DUMP_MIR' || key === 'JET_DEBUG_LOWER_ALL' || key === 'JET_KEEP_GOING_SKIP' || key === 'JET_KEEP_GOING_REPORT') delete env[key];
  // jet_rt_main runs the compiler on the process main thread and O0 frames
  // never share slots, so RLIMIT_STACK bounds recursion depth: every native
  // CLI stage execs under a 1 GiB stack set before exec.
  const stack = ['prlimit', '--stack=1073741824:1073741824', '--'];
  const built = join(unit.project, '.jet/build/selfhost');
  // Real argv CLI stage: <builder> build <assembled selfhost.jet>, same source
  // path and cwd every time, empty all-phases census, fresh outputs only.
  function cliStage(stage, builder) {
    const report = join(out, `stage${stage}-keep-going.tsv`);
    rmSync(built, { force: true }); rmSync(`${built}.o`, { force: true });
    execute(`native-stage${stage}`, [...stack, join(repo, 'Tools/agent/jet-env'), builder, 'build', unit.source], { ...env, JET_KEEP_GOING_REPORT: report }, unit.project);
    const census = readFileSync(report, 'utf8').trim().split('\n');
    if (census.length !== 1 || census[0] !== 'phase\titem\tcode\tmessage\tspan') throw Error(`stage${stage} all-phases census must exist and be empty`);
    return { ...publish(join(out, `stage${stage}`), built, `${built}.o`), keepGoingReport: report, keepGoingFailures: 0 };
  }
  // Whole-file byte comparison of jet and jet.o; never a section subset.
  const compare = (left, right) => ['jet', 'jet.o'].map(name => ({ name, equal: readFileSync(join(out, left, name)).equals(readFileSync(join(out, right, name))) }));
  const second = cliStage(2, first.binary);
  // The gate: factory stage1 == native stage2. Recorded before stage3 runs.
  const gate = compare('stage1', 'stage2');
  json(join(out, 'comparisons.json'), { fixedPoint: gate.every(row => row.equal), stages: [1, 2], files: gate, stage3: 'pending' });
  // Extra receipt: the native compiler rebuilding itself (stage2 -> stage3).
  const third = cliStage(3, second.binary);
  const selfRebuild = compare('stage2', 'stage3');
  const comparisons = { fixedPoint: gate.every(row => row.equal), stages: [1, 2], files: gate, selfRebuild: { identical: selfRebuild.every(row => row.equal), stages: [2, 3], files: selfRebuild } };
  json(join(out, 'comparisons.json'), comparisons);
  if (!comparisons.fixedPoint) throw Error('native stage1/stage2 object and ELF fixed point differ (whole-file cmp); see comparisons.json');
  if (!comparisons.selfRebuild.identical) throw Error('native stage2/stage3 self-rebuild differs (whole-file cmp); see comparisons.json');
  receipt = { schema: 'jet-native-compiler/v1', commit: record.commit, sourceRoot: repo, sourceSha256: unit.sourceSha256, factorySha256: record.factory.sha256, compilerIdentity: identity, runtimePackSha256: hash(runtime.pack), producerSha256: plan.producerSha256, harnessSha256: plan.harnessSha256, backend: 'jet-native', optimization: 'O0', fixedPoint: true, selfRebuild: true, comparisons, stages: [{ stage: 1, ...first, keepGoingReport: factory.census, keepGoingFailures: 0 }, { stage: 2, ...second }, { stage: 3, ...third }], jet: second.binary, runtimeDir: join(out, 'stage2', 'jet-runtime'), stage3: { jet: third.binary, runtimeDir: join(stage3, 'jet-runtime') }, steps, rc: 0, verdict: 'GREEN' };
} catch (error) {
  receipt = { schema: 'jet-native-feed-failure/v1', plan, steps, rc: 1, verdict: 'RED', error: error.stack, fixedPoint: false };
}
json(join(out, 'native-compiler.json'), receipt);
writeFileSync(join(out, 'driver.rc'), String(receipt.rc) + '\n');
console.log(JSON.stringify(receipt, null, 2)); process.exitCode = receipt.rc;
