#!/usr/bin/env node
// Sharded bootstrap factory -> real native CLI stage1 -> real argv stage2
// (gate: stage1 == stage2, whole files). The fixed-point receipt
// (native-compiler.json) is published as soon as that gate passes.
// `--self-rebuild --out <published dir>` is the separate follow-up: real argv
// stage3 built by stage2 (receipt self-rebuild.json: stage2 == stage3, the
// native compiler rebuilding itself); it never gates the publication.
// The factory's Rust host is a bootstrap exception, never a native compiler.
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { accessSync, chmodSync, closeSync, constants, copyFileSync, cpSync, existsSync, mkdirSync, openSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { compilerCpuPin, gnuTime, invokeStageZeroFactory, nativeAllowedRows, nativeCensusRejects, nativeCToolchain, peakRss, readStageZeroFactory } from './native-factory.mjs';
import { readNativeRuntimePack } from '../../packaging/runtime-pack-cache.mjs';
const producerRepo = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const options = { repo: producerRepo };
const usage = 'native-feed.mjs --factory-record FILE --runtime-dir DIR --out FRESH_DIR [--repo DIR] [--dry-run] | native-feed.mjs --self-rebuild --out PUBLISHED_DIR';
for (let i = 2; i < process.argv.length; i++) {
  const flag = process.argv[i];
  if (flag === '--dry-run') { options.dryRun = true; continue; }
  if (flag === '--self-rebuild') { options.selfRebuild = true; continue; }
  const key = { '--factory-record': 'factory', '--runtime-dir': 'runtime', '--out': 'out', '--repo': 'repo' }[flag];
  if (!key || !process.argv[i + 1] || process.argv[i + 1].startsWith('--')) throw Error(usage);
  options[key] = resolve(process.argv[++i]);
}
if (options.selfRebuild ? !options.out : (!options.factory || !options.runtime || !options.out)) throw Error(usage);
// The factory and every CLI stage enter the source export's jet-env, which
// refuses a CARGO_TARGET_DIR its checkout does not own (e.g. the default one a
// caller's jet-env set for a different checkout). No stage runs Cargo.
delete process.env.CARGO_TARGET_DIR;
// Compiler threads: the factory (stage1) and every native CLI stage admit
// workers from the CPUs they may run on and nothing else the chain can set
// (native-factory.mjs compilerCpuPin), so each compile whose output is
// compared runs pinned to the same NATIVE_COMPILER_CPUS CPUs (default 4: four
// threads' parse and sema scratch of the whole-compiler unit fit the feed's
// memory). A self-rebuild reuses the count its publication recorded.
const cpuCount = text => {
  if (!/^[1-9][0-9]*$/.test(text)) throw Error(`NATIVE_COMPILER_CPUS must be a positive whole number of CPUs, not ${text}`);
  return Number(text);
};
const json = (file, value) => writeFileSync(file, JSON.stringify(value, null, 2) + '\n');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const steps = [];
function execute(name, command, env, cwd, out, stepsFile) {
  const stdout = join(out, `${name}.stdout`), stderr = join(out, `${name}.stderr`);
  const start = Date.now();
  const a = openSync(stdout, 'w'), b = openSync(stderr, 'w');
  let result;
  try { result = spawnSync(command[0], command.slice(1), { env, cwd, stdio: ['ignore', a, b] }); }
  finally { closeSync(a); closeSync(b); }
  const step = { name, command, rc: result.status ?? 1, signal: result.signal, error: result.error?.message, seconds: (Date.now() - start) / 1000, stdout, stderr };
  steps.push(step); json(stepsFile, steps);
  if (step.rc) throw Error(`${name} failed; see ${stderr}`);
}
function publish(out, runtimeDir, directory, binary, object) {
  accessSync(binary, constants.X_OK);
  const bytes = readFileSync(binary), objects = readFileSync(object);
  for (const value of [bytes, objects]) if (value.subarray(0, 4).toString('hex') !== '7f454c46') throw Error('native feed did not produce both ELF object and executable');
  mkdirSync(directory, { recursive: true });
  copyFileSync(binary, join(directory, 'jet')); chmodSync(join(directory, 'jet'), 0o755);
  copyFileSync(object, join(directory, 'jet.o'));
  cpSync(runtimeDir, join(directory, 'jet-runtime'), { recursive: true, dereference: true });
  return { binary: join(directory, 'jet'), sha256: hash(bytes), objectSha256: hash(objects) };
}
// Whole-file byte comparison of jet and jet.o; never a section subset.
const compare = (out, left, right) => ['jet', 'jet.o'].map(name => ({ name, equal: readFileSync(join(out, left, name)).equals(readFileSync(join(out, right, name))) }));
// Real argv CLI stage: <builder> build <assembled selfhost.jet>, same source
// path and cwd every time, empty all-phases census, fresh outputs only.
function cliStageRunner({ repo, out, unit, identity, referenceToolchain, runtimeDir, stepsFile, cpus }) {
  const env = { ...process.env, JET_TOOLCHAIN_ROOT: repo, JET_KEEP_GOING: '1', JET_COMPILER_IDENTITY: identity };
  for (const key of Object.keys(env)) if (key.startsWith('JET_FRONT_') || key.startsWith('JET_BOOTSTRAP_') || key.startsWith('JET_TRACE_') || key === 'JET_PHASE_COUNTERS' || key === 'JET_DUMP_MIR' || key === 'JET_DEBUG_LOWER_ALL' || key === 'JET_KEEP_GOING_SKIP' || key === 'JET_KEEP_GOING_REPORT') delete env[key];
  // jet_rt_main runs the compiler on the process main thread and O0 frames
  // never share slots, so RLIMIT_STACK bounds recursion depth: every native
  // CLI stage execs under a 1 GiB stack set before exec. GNU time records
  // elapsed seconds and peak RSS; a whole-compiler stage gets at least 3 h.
  const stack = ['prlimit', '--stack=1073741824:1073741824', '--'];
  const limit = process.env.NATIVE_STAGE_TIMEOUT ?? '10800';
  if (!/^[1-9][0-9]*$/.test(limit)) throw Error('NATIVE_STAGE_TIMEOUT must be a positive number of seconds');
  const timer = gnuTime();
  const jetEnv = join(repo, 'Tools/agent/jet-env');
  // The inline-C archive (and so jet/jet.o) depends on the C toolchain the
  // route selects; a stage whose cc/ar/hardening differ from the factory's
  // cannot meet the byte gate, so refuse it before spending hours.
  const sameToolchain = (left, right) => JSON.stringify([left.cc, left.ar, left.nixHardeningEnable]) === JSON.stringify([right.cc, right.ar, right.nixHardeningEnable]);
  const built = join(unit.project, '.jet/build/selfhost');
  return (stage, builder) => {
    const report = join(out, `stage${stage}-keep-going.tsv`), timing = join(out, `stage${stage}.time`);
    // Every native CLI stage streams its own compiler trace (one event per
    // line, flushed per event) so a running stage can be watched; an inherited
    // JET_TRACE_* setting never reaches it (deleted above). Output bytes do not
    // depend on tracing.
    const stageEnv = { ...env, JET_KEEP_GOING_REPORT: report, JET_TRACE_FILE: join(out, `stage${stage}.trace.jsonl`) };
    const toolchain = nativeCToolchain(jetEnv, stageEnv, unit.project);
    if (!sameToolchain(toolchain, referenceToolchain)) throw Error(`stage${stage} C toolchain differs from the factory's: ${JSON.stringify(toolchain)} vs ${JSON.stringify(referenceToolchain)}`);
    rmSync(built, { force: true }); rmSync(`${built}.o`, { force: true });
    execute(`native-stage${stage}`, [timer, '-v', '-o', timing, 'timeout', '--kill-after=30', limit, ...stack, ...compilerCpuPin(cpus), jetEnv, builder, 'build', unit.source], stageEnv, unit.project, out, stepsFile);
    const census = readFileSync(report, 'utf8');
    const rejects = nativeCensusRejects(census, nativeAllowedRows());
    if (rejects.length) throw Error(`stage${stage} all-phases census must exist and be empty (or list only NATIVE_FEED_ALLOWED_ROWS rows): ${rejects.slice(0, 5).join(' | ')}`);
    const allowedRows = census.trim().split('\n').slice(1);
    return { ...publish(out, runtimeDir, join(out, `stage${stage}`), built, `${built}.o`), keepGoingReport: report, keepGoingFailures: allowedRows.length, allowedCensusRows: allowedRows, timing, seconds: steps.at(-1).seconds, peakRssKb: peakRss(timing), toolchain, compilerCpus: cpus };
  };
}

if (options.selfRebuild) {
  // Follow-up receipt: stage2 (the published fixed-point compiler) rebuilds
  // the same assembled source as stage3; whole-file stage2 == stage3.
  const { out } = options;
  const target = join(out, 'self-rebuild.json');
  if (existsSync(target)) throw Error(`self-rebuild receipt already exists: ${target}`);
  const publication = JSON.parse(readFileSync(join(out, 'native-compiler.json'), 'utf8'));
  if (publication.schema !== 'jet-native-compiler/v1' || publication.verdict !== 'GREEN') throw Error('self-rebuild needs a GREEN native-compiler.json (a published stage2)');
  if (!Number.isInteger(publication.compilerCpus)) throw Error('self-rebuild needs the compiler CPU count its publication recorded (compilerCpus)');
  const unit = JSON.parse(readFileSync(join(out, 'assembly', 'plan.json'), 'utf8'));
  const second = publication.stages.find(stage => stage.stage === 2);
  let selfReceipt;
  try {
    if (unit.sourceSha256 !== publication.sourceSha256) throw Error('assembled source differs from the published fixed point');
    const run = cliStageRunner({ repo: publication.sourceRoot, out, unit, identity: publication.compilerIdentity, referenceToolchain: second.toolchain, runtimeDir: publication.runtimeDir, stepsFile: join(out, 'self-rebuild-steps.json'), cpus: publication.compilerCpus });
    const third = run(3, second.binary);
    const files = compare(out, 'stage2', 'stage3');
    const identical = files.every(row => row.equal);
    selfReceipt = { schema: 'jet-native-self-rebuild/v1', commit: publication.commit, publication: join(out, 'native-compiler.json'), publicationSha256: hash(readFileSync(join(out, 'native-compiler.json'))), selfRebuild: { identical, stages: [2, 3], files }, stage3: { stage: 3, ...third, runtimeDir: join(out, 'stage3', 'jet-runtime') }, steps, rc: identical ? 0 : 1, verdict: identical ? 'GREEN' : 'RED', ...(identical ? {} : { error: 'native stage2/stage3 self-rebuild differs (whole-file cmp)' }) };
  } catch (error) {
    selfReceipt = { schema: 'jet-native-self-rebuild/v1', commit: publication.commit, publication: join(out, 'native-compiler.json'), selfRebuild: { identical: false, stages: [2, 3] }, steps, rc: 1, verdict: 'RED', error: error.stack };
  }
  json(target, selfReceipt);
  writeFileSync(join(out, 'self-rebuild.rc'), String(selfReceipt.rc) + '\n');
  console.log(JSON.stringify(selfReceipt, null, 2)); process.exitCode = selfReceipt.rc;
} else {
  const cpus = cpuCount(process.env.NATIVE_COMPILER_CPUS ?? '4');
  if (existsSync(options.out)) throw Error(`fresh native-feed output required: ${options.out}`);
  mkdirSync(options.out, { recursive: true });
  const { repo, out } = options;
  const stepsFile = join(out, 'steps.json');
  const assembly = join(out, 'assembly');
  execute('assemble', [process.execPath, join(producerRepo, 'Tools/agent/native-compiler.mjs'), '--assemble-only', '--repo', repo, '--out', assembly], process.env, repo, out, stepsFile);
  const unit = JSON.parse(readFileSync(join(assembly, 'plan.json'), 'utf8'));
  const stage1 = join(out, 'stage1');
  const prefix = join(stage1, 'factory-output');
  const plan = { schema: 'jet-native-feed-plan/v1', sourceRoot: repo, manifestPatch: unit.manifestPatch ?? null, source: unit.source, sourceSha256: unit.sourceSha256, map: unit.map, entry: unit.entry, factoryRecord: options.factory, runtimeDir: options.runtime, producerSha256: hash(readFileSync(fileURLToPath(import.meta.url))), harnessSha256: hash(readFileSync(new URL('./native-factory.mjs', import.meta.url))), stages: ['source-bound AOT factory -> native stage1', 'native stage1 real argv -> native stage2'], comparisons: ['stage1/jet.o == stage2/jet.o', 'stage1/jet == stage2/jet'], selfRebuild: { followUp: 'native-feed.mjs --self-rebuild --out <this dir>', receipt: 'self-rebuild.json', comparisons: ['stage2/jet.o == stage3/jet.o', 'stage2/jet == stage3/jet'] }, stack: '1 GiB RLIMIT_STACK (prlimit) for every native CLI stage', compilerCpus: cpus, allPhasesReportsMandatory: true, optimization: 'O0' };
  json(join(out, 'plan.json'), plan);
  if (options.dryRun) { console.log(JSON.stringify({ ...plan, dryRun: true, upstreamArtifactsNotYetAdmitted: true }, null, 2)); process.exit(0); }
  let receipt;
  try {
    const record = readStageZeroFactory(options.factory, repo);
    // The native CLI takes its compiler identity only from the environment
    // (JetCli Request.jet jet_cli_driver_request) and refuses to compile without
    // it (Pipeline.jet E0003). Every native stage must use the identity the
    // factory baked in (Bootstrap/Host/Native.rs, `jet@<version>#<source id>`),
    // which the factory's own bootstrap receipt records (Bootstrap/Tests.rs).
    const runtime = readNativeRuntimePack(options.runtime);
    if (runtime.provenance.bootstrapFactory?.sourceCommit !== record.commit || runtime.provenance.compilerSha256 !== record.factory.sha256) throw Error('runtime pack must be produced by this same source-bound factory');
    const packPath = join(options.runtime, runtime.key, 'jet_runtime.pack.o');
    mkdirSync(stage1);
    const start = Date.now();
    const factory = invokeStageZeroFactory({ recordFile: options.factory, repo, sourceRoot: unit.project, entry: 'selfhost.jet', work: join(out, 'factory-native'), policy: { JET_FRONT_NATIVE_OUTPUT: prefix, JET_RUNTIME_PACK: packPath }, cpus });
    for (const suffix of ['.issues', '.pack-issues']) {
      const file = prefix + suffix;
      if (existsSync(file) && readFileSync(file, 'utf8').trim()) throw Error(`native stage1 codegen/link findings: ${file}`);
    }
    const receiptRows = existsSync(factory.receipt) ? readFileSync(factory.receipt, 'utf8').split('\n') : [];
    const identity = receiptRows.find(row => row.startsWith('compiler_source_identity='))?.slice('compiler_source_identity='.length);
    if (typeof identity !== 'string' || !/^[^@#\s]+@[^@#\s]+#[^@#\s]+$/.test(identity)) throw Error(`factory receipt ${factory.receipt} lacks compiler_source_identity (\`name@version#source-id\`); refusing to invent one`);
    const first = publish(out, options.runtime, stage1, prefix + '.exe', prefix + '.o');
    steps.push({ name: 'factory-native-stage1', rc: 0, seconds: (Date.now() - start) / 1000, factorySeconds: factory.seconds, peakRssKb: factory.peakRssKb, timing: factory.timing, toolchain: factory.toolchain, factoryReceipt: factory.receipt, keepGoingReport: factory.census, ...first });
    json(stepsFile, steps);
    const artifactsFile = join(dirname(repo), 'ARTIFACTS.json');
    const artifacts = existsSync(artifactsFile) ? JSON.parse(readFileSync(artifactsFile, 'utf8')) : { schema: 'jet-regression-artifacts/v1', regression_role: 'discovery', commit: record.commit, tree: record.tree, source: repo, target_dir: `/mnt/jetscratch/targets/source-${record.commit}`, artifacts: {} };
    if (artifacts.schema !== 'jet-regression-artifacts/v1' || artifacts.commit !== record.commit || artifacts.tree !== record.tree || artifacts.source !== repo) throw Error('artifact publication source pin mismatch');
    artifacts.artifacts.candidate_jetc0 = { path: first.binary, sha256: first.sha256, native: true, runtimeDir: join(stage1, 'jet-runtime'), sourceSha256: unit.sourceSha256 };
    const temporaryArtifacts = `${artifactsFile}.${process.pid}.new`;
    json(temporaryArtifacts, artifacts); renameSync(temporaryArtifacts, artifactsFile);
    const run = cliStageRunner({ repo, out, unit, identity, referenceToolchain: factory.toolchain, runtimeDir: options.runtime, stepsFile, cpus });
    const second = run(2, first.binary);
    // Owner decision 2026-10-16: the self-hosting gate is stage2 == stage3
    // (the `--self-rebuild` follow-up). Factory stage1 vs native stage2 is
    // recorded (whole files) and fixed after self-hosting; it does not gate.
    const gate = compare(out, 'stage1', 'stage2');
    const comparisons = { fixedPoint: gate.every(row => row.equal), stages: [1, 2], files: gate };
    json(join(out, 'comparisons.json'), comparisons);
    // `stages` stays the stage1/stage2 gate pair (native-smoke admits exactly
    // two); smoke uses stage2/jet.
    receipt = { schema: 'jet-native-compiler/v1', commit: record.commit, sourceRoot: repo, sourceSha256: unit.sourceSha256, manifestPatch: plan.manifestPatch, factorySha256: record.factory.sha256, compilerIdentity: identity, runtimePackSha256: hash(runtime.pack), producerSha256: plan.producerSha256, harnessSha256: plan.harnessSha256, backend: 'jet-native', optimization: 'O0', compilerCpus: cpus, fixedPoint: comparisons.fixedPoint, gate: 'stage2==stage3', comparisons, selfRebuildReceipt: join(out, 'self-rebuild.json'), stages: [{ stage: 1, ...first, keepGoingReport: factory.census, keepGoingFailures: Number(factory.admitted?.allowed_census_rows ?? 0), compilerCpus: cpus }, { stage: 2, ...second }], jet: second.binary, runtimeDir: join(out, 'stage2', 'jet-runtime'), steps, rc: 0, verdict: 'GREEN' };
  } catch (error) {
    receipt = { schema: 'jet-native-feed-failure/v1', plan, steps, rc: 1, verdict: 'RED', error: error.stack, fixedPoint: false };
  }
  json(join(out, 'native-compiler.json'), receipt);
  writeFileSync(join(out, 'driver.rc'), String(receipt.rc) + '\n');
  console.log(JSON.stringify(receipt, null, 2)); process.exitCode = receipt.rc;
}
