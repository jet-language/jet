#!/usr/bin/env node
// The compile-throughput cell's harness (Tower #4529; the cell is
// axes.compile_throughput in Tools/gauntlet/measurement-manifest.json).
// Compiles the cell's programs clean with one compiler binary, in a default
// jobs arm and a single-job arm, and writes one receipt with wall, CPU, peak
// RSS, threads and per-phase nanoseconds per sample, the program's line
// shape, and the compiler's identity and build profile. Any failed,
// incomplete, wrong-output or nondeterministic sample fails the run (exit 1).
// `--help` prints the options; README.md explains the lanes.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { generateMixed } from '../scaling/generate.mjs';
import { countLines } from '../scaling/mixed.mjs';
import { parseCounters, phaseTable, readTrace, runMeasured } from './measure.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
export const REPO = path.resolve(here, '../../..');
export const PINS = JSON.parse(fs.readFileSync(path.join(here, 'bench.json'), 'utf8'));
const MANIFEST = path.join(REPO, 'Tools/gauntlet/measurement-manifest.json');
const PERF_BASELINE = path.join(REPO, 'Tools/perf/baseline.json');
export const RECEIPT_SCHEMA = 'jet.compile-throughput.receipt/v1';
export const QUICK_BASELINE_SCHEMA = 'jet.compile-throughput.quick-baseline/v1';
// Baseline values below these floors are under the sampler's, the
// scheduler's and the allocator's resolution; the 15% budgets apply to every
// phase, the whole frontend and peak RSS at or above them.
export const QUICK_PHASE_FLOOR_NS = 100_000_000;
export const QUICK_MEMORY_FLOOR_BYTES = 64 * 1024 * 1024;

const USAGE = `Usage: node Tools/perf/throughput/harness.mjs --compiler KIND=PATH [options]
  KIND is bootstrap (a generated Jet compiler: jetc0, jetc1, ...; driven by the
  JET_BOOTSTRAP_* runner protocol) or jet (the \`jet build\` command).
  --lane quick|full        quick (default): bench30k, frontend only, 3 samples,
                           checked against --baseline with Tools/perf/baseline.json's
                           latency/memory regression budgets. full: the manifest cell.
  --programs a,b           bench300k, bench30k, selfhost (default: the lane's)
  --selfhost-rung L1..L5   selfhost unit (default L5, the whole compiler); other
                           rungs are recorded as selfhost-<rung>, never as selfhost
  --source-tree DIR        compiler sources for selfhost (default: this checkout)
  --samples N --warmups N  per arm (default: lane's samples, 1 warmup)
  --arms default,single    arms to run (default both)
  --single-control SPEC    how the single arm forces one job: env:NAME=VALUE or
                           arg:FLAG (none until D-JOBS1; the arm is then recorded
                           as uncontrolled and still checked for determinism)
  --backend | --no-backend build and run the artifact (default: full on, quick off)
  --keep DIR               retained stage-zero dir for bootstrap backend builds
                           (default: the compiler's directory)
  --compiler-profile TEXT  build profile when it cannot be read from the build
  --memory-max SIZE        systemd scope cap per measured command (default 12G)
  --timeout SECONDS        per measured command (default 14400)
  --work-dir DIR           (default ~/.cache/jet-dev/scratch/throughput)
  --receipt FILE           (default <work-dir>/<run>/receipt.json)
  --baseline FILE          quick-lane baseline (default Tools/perf/throughput/quick-baseline.json)
  --write-baseline FILE    write this run's quick-lane baseline instead of comparing`;

function options(args) {
  const config = {
    lane: 'quick', compiler: null, programs: null, rung: 'L5', sourceTree: REPO, samples: null, warmups: 1,
    arms: ['default', 'single'], singleControl: null, backend: null, keep: null, profile: null,
    memoryMax: '12G', timeoutMs: 14400 * 1000, workDir: path.join(os.homedir(), '.cache/jet-dev/scratch/throughput'),
    receipt: null, baseline: path.join(here, 'quick-baseline.json'), writeBaseline: null,
  };
  for (let i = 0; i < args.length; i++) {
    const flag = args[i];
    if (flag === '--help') return null;
    if (flag === '--backend' || flag === '--no-backend') { config.backend = flag === '--backend'; continue; }
    const value = args[++i];
    if (value === undefined) throw new Error(`Missing value for ${flag}`);
    switch (flag) {
      case '--lane': config.lane = value; break;
      case '--compiler': {
        const split = value.indexOf('=');
        config.compiler = { kind: value.slice(0, split), path: path.resolve(value.slice(split + 1)) };
        break;
      }
      case '--programs': config.programs = value.split(','); break;
      case '--selfhost-rung': config.rung = value; break;
      case '--source-tree': config.sourceTree = path.resolve(value); break;
      case '--samples': config.samples = Number(value); break;
      case '--warmups': config.warmups = Number(value); break;
      case '--arms': config.arms = value.split(','); break;
      case '--single-control': config.singleControl = value; break;
      case '--keep': config.keep = path.resolve(value); break;
      case '--compiler-profile': config.profile = value; break;
      case '--memory-max': config.memoryMax = value; break;
      case '--timeout': config.timeoutMs = Number(value) * 1000; break;
      case '--work-dir': config.workDir = path.resolve(value); break;
      case '--receipt': config.receipt = path.resolve(value); break;
      case '--baseline': config.baseline = path.resolve(value); break;
      case '--write-baseline': config.writeBaseline = path.resolve(value); break;
      default: throw new Error(`Unknown option: ${flag}`);
    }
  }
  if (!['quick', 'full'].includes(config.lane)) throw new Error('--lane must be quick or full');
  if (!config.compiler || !['bootstrap', 'jet'].includes(config.compiler.kind)) throw new Error('--compiler must be bootstrap=PATH or jet=PATH');
  if (!fs.existsSync(config.compiler.path)) throw new Error(`No compiler at ${config.compiler.path}`);
  config.programs ??= config.lane === 'quick' ? ['bench30k'] : ['bench300k', 'selfhost'];
  for (const program of config.programs) if (!['bench300k', 'bench30k', 'selfhost'].includes(program)) throw new Error(`Unknown program: ${program}`);
  if (!/^L[1-5]$/.test(config.rung)) throw new Error('--selfhost-rung must be L1..L5');
  config.samples ??= config.lane === 'quick' ? 3 : 5;
  config.backend ??= config.lane === 'full';
  if (!Number.isSafeInteger(config.samples) || config.samples < 1) throw new Error('--samples must be a positive integer');
  if (!Number.isSafeInteger(config.warmups) || config.warmups < 0) throw new Error('--warmups must be a nonnegative integer');
  if (!config.arms.length || config.arms.some(arm => !['default', 'single'].includes(arm))) throw new Error('--arms takes default and/or single');
  if (config.singleControl !== null && !/^(env:[A-Z_][A-Z0-9_]*=.*|arg:\S+)$/.test(config.singleControl)) throw new Error('--single-control must be env:NAME=VALUE or arg:FLAG');
  if (!/^\d+[KMG]$/.test(config.memoryMax)) throw new Error('--memory-max must look like 12G');
  return config;
}

const sha256 = data => crypto.createHash('sha256').update(data).digest('hex');
export function fileSha256(file) {
  const hash = crypto.createHash('sha256');
  const fd = fs.openSync(file, 'r');
  const buffer = Buffer.allocUnsafe(1 << 22);
  try {
    for (;;) {
      const read = fs.readSync(fd, buffer, 0, buffer.length, null);
      if (!read) break;
      hash.update(buffer.subarray(0, read));
    }
  } finally {
    fs.closeSync(fd);
  }
  return hash.digest('hex');
}
const median = values => {
  const sorted = [...values].sort((a, b) => a - b);
  const middle = sorted.length >> 1;
  return sorted.length % 2 ? sorted[middle] : Math.round((sorted[middle - 1] + sorted[middle]) / 2);
};
const run = (command, args, opts = {}) => spawnSync(command, args, { encoding: 'utf8', maxBuffer: 64 << 20, ...opts });

export function machine() {
  const cpuinfo = fs.readFileSync('/proc/cpuinfo', 'utf8');
  const cores = new Set([...cpuinfo.matchAll(/physical id\s*:\s*(\d+)[\s\S]*?core id\s*:\s*(\d+)/g)].map(match => `${match[1]}:${match[2]}`));
  const read = file => { try { return fs.readFileSync(file, 'utf8').trim(); } catch { return null; } };
  return {
    cpu_model: /model name\s*:\s*(.+)/.exec(cpuinfo)?.[1].trim() ?? null,
    cores: cores.size || null,
    threads: os.cpus().length,
    memory_bytes: Number(/MemTotal:\s+(\d+) kB/.exec(read('/proc/meminfo'))[1]) * 1024,
    governor: read('/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor'),
    kernel: os.release(),
    hostname: os.hostname(),
  };
}

// What else is running: load and every other process above 1 GiB RSS, so a
// receipt taken on a busy machine says so.
function contention() {
  const heavy = [];
  for (const entry of fs.readdirSync('/proc')) {
    if (!/^\d+$/.test(entry) || Number(entry) === process.pid) continue;
    try {
      const status = fs.readFileSync(`/proc/${entry}/status`, 'utf8');
      const rss = Number(/VmRSS:\s+(\d+) kB/.exec(status)?.[1] ?? 0) * 1024;
      if (rss >= 1 << 30) heavy.push({ pid: Number(entry), name: /Name:\s+(\S+)/.exec(status)?.[1], rss_bytes: rss });
    } catch { /* exited */ }
  }
  return { load1: Number(fs.readFileSync('/proc/loadavg', 'utf8').split(' ')[0]), heavy_processes: heavy };
}

function commit() {
  const head = run('git', ['-C', REPO, 'rev-parse', 'HEAD']).stdout.trim();
  const dirty = run('git', ['-C', REPO, 'status', '--porcelain', '--untracked-files=no']).stdout.trim() !== '';
  return { head, dirty };
}

// The compiler's build profile, read from the build that produced it.
// Generated compilers: the backend Cargo manifest beside the binary's build
// (stage zero's for jetc0, the stage's backend project for jetc1+): every
// [profile.*] and per-package opt-level row, summarized and digested.
export function buildProfile(compiler, override) {
  if (override) return { source: 'operator', description: override };
  const dir = path.dirname(compiler.path);
  const candidates = compiler.kind === 'bootstrap'
    ? [path.join(dir, 'stage-zero/Cargo.toml'), path.join(dir, '../backend-jetc1/Cargo.toml'), path.join(dir, `../backend-${path.basename(compiler.path)}/Cargo.toml`)]
    : [];
  for (const manifest of candidates) {
    if (!fs.existsSync(manifest)) continue;
    const text = fs.readFileSync(manifest, 'utf8');
    const profiles = {};
    let current = null;
    for (const line of text.split('\n')) {
      const header = /^\[profile\.([^.\]]+)(?:\.package\.([^\]]+))?\]/.exec(line);
      if (header) { current = { profile: header[1], package: header[2] ?? '*' }; continue; }
      if (/^\[/.test(line)) { current = null; continue; }
      const opt = /^opt-level\s*=\s*("?)(\w+)\1/.exec(line);
      if (current && opt) {
        const row = (profiles[current.profile] ??= { default_opt_level: null, package_opt_levels: {} });
        if (current.package === '*') row.default_opt_level = opt[2];
        else row.package_opt_levels[opt[2]] = (row.package_opt_levels[opt[2]] ?? 0) + 1;
      }
    }
    const envFile = path.join(dir, 'jetc0.env');
    const pkg = fs.existsSync(envFile) ? /^package=(.*)$/m.exec(fs.readFileSync(envFile, 'utf8'))?.[1] ?? null : null;
    const binaryDir = path.basename(path.dirname(compiler.path));
    return { source: path.relative(dir, manifest), manifest_sha256: sha256(text), package: pkg, cargo_profile_dir: binaryDir, profiles };
  }
  if (compiler.kind === 'jet' && /\/release\/[^/]+$/.test(compiler.path)) {
    return { source: 'cargo target directory', description: 'cargo --release (workspace Cargo.toml [profile.release])', cargo_profile_dir: 'release' };
  }
  throw new Error(`Cannot read the build profile of ${compiler.path}; pass --compiler-profile`);
}

function prepareBench(id, dir) {
  const pin = PINS.programs[id];
  if (!pin) throw new Error(`bench.json has no pin for ${id}`);
  fs.rmSync(dir, { recursive: true, force: true });
  const generated = generateMixed(pin.code_lines, pin.seed, dir);
  const failures = [];
  if (generated.source_sha256 !== pin.source_sha256) failures.push(`${id}: generated source ${generated.source_sha256} differs from the pinned ${pin.source_sha256}`);
  if (generated.expected !== pin.expected_output) failures.push(`${id}: generated golden ${JSON.stringify(generated.expected)} differs from the pinned ${JSON.stringify(pin.expected_output)}`);
  if (generated.stats.code_lines < pin.code_lines) failures.push(`${id}: ${generated.stats.code_lines} code lines < ${pin.code_lines}`);
  return {
    failures, entry: 'main.jet', expected: pin.expected_output,
    source: { kind: 'generated', generator: `node Tools/perf/scaling/generate.mjs mixed --code-lines ${pin.code_lines} --seed ${pin.seed}`, sha256: generated.source_sha256, ...generated.stats },
  };
}

function prepareSelfhost(config, dir) {
  fs.rmSync(dir, { recursive: true, force: true });
  const rungs = path.join(dir, 'rungs');
  const result = run(process.execPath, [path.join(REPO, 'Tools/agent/stage1/ladder.mjs'), 'rungs', config.sourceTree, rungs]);
  if (result.status !== 0) throw new Error(`ladder.mjs rungs failed: ${result.stderr.trim().split('\n').slice(-3).join(' | ')}`);
  const project = path.join(rungs, config.rung, 'project');
  const unit = fs.readFileSync(path.join(project, 'src/compiler.jet'));
  const lines = countLines(unit.toString('utf8'));
  return {
    failures: [], entry: 'src/compiler.jet', expected: null, project,
    source: {
      kind: 'assembled', rung: config.rung, source_tree: config.sourceTree, sha256: sha256(unit),
      physical_lines: lines.physical, code_lines: lines.code, bytes: unit.length,
      functions: (unit.toString('utf8').match(/^\s*(pub )?fn /gm) ?? []).length,
    },
  };
}

// Receipt and diagnostics of one bootstrap runner compile.
function bootstrapOutcome(sample) {
  const text = fs.existsSync(sample.receiptPath) ? fs.readFileSync(sample.receiptPath, 'utf8') : '';
  const field = name => new RegExp(`^${name}=(.*)$`, 'm').exec(text)?.[1] ?? null;
  const reports = text.split('\n').filter(line => line.startsWith('report_json=')).map(line => line.slice('report_json='.length));
  const errors = reports.map(line => { try { return JSON.parse(line); } catch { return { severity: 'error', what: 'unparsable report' }; } })
    .filter(report => report.severity === 'error');
  const problems = [];
  if (!text) problems.push('no compiler receipt');
  else {
    if (field('mode') !== 'runner') problems.push(`receipt mode ${field('mode')}`);
    if (field('complete') !== 'true') problems.push('receipt complete=false');
    if (!(Number(field('source_bytes')) > 0)) problems.push('receipt source_bytes=0');
    if (!fs.existsSync(sample.outputPath)) problems.push('no emitted output');
  }
  for (const error of errors.slice(0, 3)) problems.push(`${error.code ?? ''} ${error.what ?? ''} (${path.basename(error.file ?? '')}:${error.line ?? ''})`.trim());
  return {
    problems,
    artifact: fs.existsSync(sample.outputPath) ? sample.outputPath : null,
    diagnostics: reports.join('\n'),
    compiler_identity: field('compiler_identity'),
    report_count: Number(field('report_count') ?? 0),
  };
}

// `jet build` publishes no artifact path, so the fresh project's .jet/build
// must hold exactly one executable.
function jetOutcome(sample, status) {
  const problems = [];
  if (status !== 0) problems.push(`jet build exited ${status}`);
  const buildDir = path.join(sample.project, '.jet/build');
  const executables = fs.existsSync(buildDir) ? fs.readdirSync(buildDir)
    .map(name => path.join(buildDir, name)).filter(file => fs.statSync(file).isFile() && (fs.statSync(file).mode & 0o111) && !file.endsWith('.rs')) : [];
  if (status === 0 && executables.length !== 1) problems.push(`.jet/build holds ${executables.length} executables, expected 1`);
  const stderr = fs.readFileSync(sample.stderrPath, 'utf8');
  return { problems, artifact: executables.length === 1 ? executables[0] : null, executable: executables[0] ?? null, diagnostics: stderr };
}

function scoped(config, argv) {
  return ['systemd-run', '--user', '--scope', '-q', '-p', `MemoryMax=${config.memoryMax}`, '-p', 'MemorySwapMax=0',
    'bash', '-c', 'ulimit -c 0; ulimit -s 1048576; exec "$@"', 'scoped', ...argv];
}

function singleControl(config, arm) {
  if (arm !== 'single' || !config.singleControl) return { env: {}, args: [] };
  const [kind, value] = [config.singleControl.slice(0, 3), config.singleControl.slice(4)];
  if (kind === 'env') { const split = value.indexOf('='); return { env: { [value.slice(0, split)]: value.slice(split + 1) }, args: [] }; }
  return { env: {}, args: [value] };
}

async function compileSample(config, program, arm, dir) {
  const sample = {
    project: path.join(dir, 'project'), store: path.join(dir, 'store'), tracePath: path.join(dir, 'trace.json'),
    outputPath: path.join(dir, 'out.rs'), receiptPath: path.join(dir, 'receipt'),
    stdoutPath: path.join(dir, 'compile.stdout'), stderrPath: path.join(dir, 'compile.stderr'),
  };
  // The clean lane: one fresh copy of the program at the same path for every
  // sample of every arm (emitted text may name source paths), an empty
  // program store, no .jet state, no daemon. Copying is outside the measured
  // interval.
  const fixed = path.join(path.dirname(path.dirname(dir)), 'project');
  fs.rmSync(fixed, { recursive: true, force: true });
  fs.cpSync(program.project, fixed, { recursive: true });
  sample.project = fixed;
  fs.mkdirSync(sample.store, { recursive: true });
  const control = singleControl(config, arm);
  let argv;
  let env;
  if (config.compiler.kind === 'bootstrap') {
    env = { HOME: os.homedir(), PATH: process.env.PATH, TMPDIR: dir, LC_ALL: 'C',
      JET_BOOTSTRAP_MODE: 'runner', JET_BOOTSTRAP_FACTORY_TIER: 'aot', JET_BOOTSTRAP_SOURCE_ROOT: sample.project,
      JET_BOOTSTRAP_ENTRY: program.entry, JET_BOOTSTRAP_OUTPUT: sample.outputPath, JET_BOOTSTRAP_RECEIPT: sample.receiptPath,
      JET_STORE_DIR: sample.store, RUST_MIN_STACK: '8388608', JET_TRACE_FILE: sample.tracePath, ...control.env };
    argv = scoped(config, ['env', '-i', ...Object.entries(env).map(([key, value]) => `${key}=${value}`), config.compiler.path, ...control.args]);
  } else {
    env = { ...process.env, JET_STORE_DIR: sample.store, JET_TRACE_FILE: sample.tracePath, NO_COLOR: '1', TMPDIR: dir, ...control.env };
    argv = scoped(config, [config.compiler.path, 'build', '--color=never', ...control.args, program.entry]);
  }
  // systemd-run reaches the user manager through the caller's environment;
  // a bootstrap compiler sees only the explicit `env -i` list.
  const spawnEnv = config.compiler.kind === 'bootstrap' ? process.env : env;
  const measured = await runMeasured(argv, { cwd: sample.project, env: spawnEnv, stdoutPath: sample.stdoutPath, stderrPath: sample.stderrPath, timeoutMs: config.timeoutMs });
  const outcome = config.compiler.kind === 'bootstrap' ? bootstrapOutcome(sample) : jetOutcome(sample, measured.code);
  if (measured.killed) outcome.problems.unshift(`timed out after ${config.timeoutMs / 1000}s`);
  else if (measured.code !== 0) outcome.problems.unshift(`compiler exited ${measured.code}${measured.signal ? ` (${measured.signal})` : ''}`);
  if (!measured.rusage) outcome.problems.push('no rusage from GNU time');
  return { sample, measured, outcome };
}

// Build (bootstrap) and run the artifact; validate stdout byte-exactly.
async function backendAndRun(config, program, arm, dir, compiled) {
  const result = { backend: null, run: null, problems: [] };
  let binary = compiled.outcome.executable ?? null;
  if (config.compiler.kind === 'bootstrap') {
    const keep = config.keep ?? path.dirname(config.compiler.path);
    const jobs = arm === 'single' ? 1 : os.cpus().length;
    // A fresh package name per sample makes cargo rebuild the program crate
    // while the shipped runtime crates stay built (toolchain contents).
    const pkg = `throughput_${program.id.replace(/\W/g, '_')}_${path.basename(dir).replace(/\W/g, '_')}`;
    const selfhost = program.id.startsWith('selfhost');
    const script = [
      'set -u', 'export JETC0_KEEP="$1"', '. "$2/Tools/agent/stage1/lib.sh"', 'load_keep >&2',
      'source=$3', `if [ "${selfhost ? 1 : 0}" = 1 ]; then with_artifact_main "$3" "\${3%.rs}.main.rs"; source=\${3%.rs}.main.rs; fi`,
      'bin=$(build_backend "$4" "$5" "$source" "$6") || exit 1', 'printf "%s\\n" "$bin" > "$7"',
    ].join('\n');
    const binFile = path.join(dir, 'backend.bin');
    const env = { ...process.env, STAGE1_CARGO_JOBS: String(jobs), STAGE1_CARGO_MEM: config.memoryMax };
    const measured = await runMeasured(['bash', '-c', script, 'backend', keep, REPO, compiled.outcome.artifact, path.join(dir, 'backend'), pkg, path.join(dir, 'backend.log'), binFile],
      { cwd: dir, env, stdoutPath: path.join(dir, 'backend.stdout'), stderrPath: path.join(dir, 'backend.stderr'), timeoutMs: config.timeoutMs });
    const log = fs.existsSync(path.join(dir, 'backend.log')) ? fs.readFileSync(path.join(dir, 'backend.log'), 'utf8') : '';
    const cargo = /Finished `[^`]+` profile .* in (?:(\d+)m )?(\d+(?:\.\d+)?)s/.exec(log);
    result.backend = {
      wall_ns: measured.wallNs, cpu_user_ns: measured.rusage?.userNs ?? null, cpu_sys_ns: measured.rusage?.sysNs ?? null,
      peak_rss_bytes: measured.peakRssBytes, threads_used: measured.threadsUsed, jobs,
      cargo_reported_ns: cargo ? Math.round(((Number(cargo[1] ?? 0) * 60) + Number(cargo[2])) * 1e9) : null,
    };
    if (measured.code !== 0 || !fs.existsSync(binFile)) { result.problems.push(`backend build failed (exit ${measured.code}); see ${dir}/backend.stderr and backend.log`); return result; }
    binary = fs.readFileSync(binFile, 'utf8').trim();
  }
  if (program.expected === null) return result;
  if (!binary) { result.problems.push('no executable to run'); return result; }
  const ran = run(binary, [], { cwd: dir, timeout: 120_000, env: { ...process.env, NO_COLOR: '1' } });
  result.run = { exit: ran.status, stdout_sha256: sha256(ran.stdout ?? ''), stdout: ran.stdout };
  if (ran.status !== 0) result.problems.push(`artifact exited ${ran.status}`);
  if (ran.stdout !== program.expected) result.problems.push(`artifact stdout ${JSON.stringify((ran.stdout ?? '').slice(0, 200))} differs from the golden ${JSON.stringify(program.expected)}`);
  return result;
}

function sampleRecord(compiled, backend, programRoot) {
  const { measured, outcome, sample } = compiled;
  const events = fs.existsSync(sample.tracePath) ? readTrace(sample.tracePath) : [];
  const start = events.find(event => event.name === 'trace.start');
  const offsetNs = start ? (start.args.unix_ms - measured.spawnUnixMs) * 1e6 : 0;
  const { phases, firstBeginNs, unclosed } = phaseTable(events, measured.samples, offsetNs);
  const problems = [...outcome.problems, ...(backend?.problems ?? [])];
  if (!events.length) problems.push('no JET_TRACE_FILE trace');
  if (unclosed.length) problems.push(`trace spans left open: ${[...new Set(unclosed)].join(', ')}`);
  const normalize = text => text.split(programRoot).join('<project>');
  const record = {
    wall_ns: measured.wallNs + (backend?.backend?.wall_ns ?? 0),
    frontend: {
      wall_ns: measured.wallNs, cpu_user_ns: measured.rusage?.userNs ?? null, cpu_sys_ns: measured.rusage?.sysNs ?? null,
      peak_rss_bytes: measured.peakRssBytes, threads_used: measured.threadsUsed,
      startup_ns: firstBeginNs === null ? null : Math.round(firstBeginNs),
    },
    backend: backend?.backend ?? null,
    cpu_user_ns: (measured.rusage?.userNs ?? 0) + (backend?.backend?.cpu_user_ns ?? 0),
    cpu_sys_ns: (measured.rusage?.sysNs ?? 0) + (backend?.backend?.cpu_sys_ns ?? 0),
    peak_rss_bytes: Math.max(measured.peakRssBytes, backend?.backend?.peak_rss_bytes ?? 0),
    threads_used: Math.max(measured.threadsUsed, backend?.backend?.threads_used ?? 0),
    phases,
    phase_counters: parseCounters(`${fs.readFileSync(sample.stdoutPath, 'utf8')}\n${fs.readFileSync(sample.stderrPath, 'utf8')}`),
    artifact_sha256: outcome.artifact ? fileSha256(outcome.artifact) : null,
    diagnostic_sha256: sha256(normalize(outcome.diagnostics)),
    report_count: outcome.report_count ?? null,
    compiler_identity: outcome.compiler_identity ?? null,
    run: backend?.run ? { exit: backend.run.exit, stdout_sha256: backend.run.stdout_sha256 } : null,
    problems,
  };
  return record;
}

const SUMMARY_KEYS = ['wall_ns', 'cpu_user_ns', 'cpu_sys_ns', 'peak_rss_bytes', 'threads_used'];
export function summarize(samples) {
  const summary = {};
  for (const key of SUMMARY_KEYS) {
    const values = samples.map(sample => sample[key]);
    summary[key] = { median: median(values), min: Math.min(...values), max: Math.max(...values) };
  }
  summary.frontend_wall_ns = { median: median(samples.map(sample => sample.frontend.wall_ns)) };
  if (samples.every(sample => sample.backend)) summary.backend_wall_ns = { median: median(samples.map(sample => sample.backend.wall_ns)) };
  const kinds = [...new Set(samples.flatMap(sample => Object.keys(sample.phases)))].sort();
  summary.phases = {};
  for (const kind of kinds) {
    const present = samples.filter(sample => sample.phases[kind]);
    summary.phases[kind] = {
      wall_ns: median(present.map(sample => sample.phases[kind].wall_ns)),
      cpu_ns: median(present.map(sample => sample.phases[kind].cpu_ns)),
      samples: present.length,
    };
  }
  return summary;
}

// SPEED-PLAN.md section 1.3 wall budgets for 300,000 lines; each row owns
// trace phase kinds and names the cards whose work brings it in budget.
export function phaseBudgetCheck(summary, codeLines) {
  const scale = codeLines / 300000;
  const rows = [];
  const total = summary.frontend_wall_ns.median;
  let named = 0;
  for (const row of PINS.phase_budgets) {
    let actual;
    if (row.measure === 'startup') actual = summary.startup_ns ?? null;
    else if (row.measure === 'backend') actual = summary.backend_wall_ns?.median ?? null;
    else if (row.measure === 'glue') actual = null;
    else actual = row.kinds.reduce((sum, kind) => sum + (summary.phases[kind]?.wall_ns ?? 0), 0);
    if (row.kinds) named += actual ?? 0;
    rows.push({ row: row.row, phase: row.phase, budget_ns: Math.round(row.wall_budget_s * 1e9 * scale), actual_ns: actual, owners: row.owners });
  }
  const glue = rows.find(row => PINS.phase_budgets.find(budget => budget.row === row.row).measure === 'glue');
  if (glue) glue.actual_ns = Math.max(0, total - named - (summary.startup_ns ?? 0));
  for (const row of rows) row.status = row.actual_ns === null ? 'unmeasured' : row.actual_ns < row.budget_ns ? 'within' : 'over';
  return { scale, rows, over: rows.filter(row => row.status !== 'within').map(row => `${row.phase}: ${row.actual_ns} ns vs ${row.budget_ns} ns budget (owners ${row.owners.join(', ')})`) };
}

// The cell's gates (manifest axes.compile_throughput.gates and thresholds).
export function cellGates(cell, programs) {
  const failures = [];
  const results = {};
  for (const program of programs) {
    const gate = {};
    const arms = program.arms;
    const digests = new Set(arms.flatMap(arm => arm.samples.map(sample => `${sample.artifact_sha256}/${sample.diagnostic_sha256}`)));
    gate.determinism = digests.size === 1 ? 'pass' : 'fail';
    if (digests.size !== 1) failures.push(`${program.id}: artifacts or diagnostics differ across samples and arms (${digests.size} distinct)`);
    const contract = cell.programs.find(row => row.id === program.id);
    const memoryKey = `${program.id}_peak_rss_bytes_max`;
    const defaultArm = arms.find(arm => arm.id === 'default');
    if (contract && cell.gates.memory[memoryKey] !== undefined && defaultArm) {
      const peak = defaultArm.summary.peak_rss_bytes.max;
      gate.memory = peak <= cell.gates.memory[memoryKey] ? 'pass' : 'fail';
      if (gate.memory === 'fail') failures.push(`${program.id}: peak RSS ${peak} B > ${cell.gates.memory[memoryKey]} B`);
    }
    if (contract && defaultArm) {
      const wall = defaultArm.summary.wall_ns;
      if (contract.threshold.wall_ns_lt !== undefined) {
        gate.threshold = wall.median < contract.threshold.wall_ns_lt && wall.max < contract.threshold.wall_ns_lt ? 'pass' : 'fail';
        if (gate.threshold === 'fail') failures.push(`${program.id}: wall median ${wall.median} ns / max ${wall.max} ns, threshold < ${contract.threshold.wall_ns_lt} ns`);
      } else if (contract.threshold.lines_per_second_min !== undefined) {
        const rate = ns => program.source.physical_lines / (ns / 1e9);
        gate.lines_per_second = { median: Math.round(rate(wall.median)), min: Math.round(rate(wall.max)) };
        gate.threshold = gate.lines_per_second.median >= contract.threshold.lines_per_second_min && gate.lines_per_second.min >= contract.threshold.lines_per_second_min ? 'pass' : 'fail';
        if (gate.threshold === 'fail') failures.push(`${program.id}: ${gate.lines_per_second.median} lines/s median, ${gate.lines_per_second.min} min, threshold >= ${contract.threshold.lines_per_second_min}`);
      }
      const budgets = phaseBudgetCheck({ ...defaultArm.summary, startup_ns: median(defaultArm.samples.map(sample => sample.frontend.startup_ns ?? 0)) }, program.source.code_lines);
      gate.phase_budgets = budgets;
      for (const over of budgets.over) failures.push(`${program.id}: phase over budget: ${over}`);
    }
    results[program.id] = gate;
  }
  return { results, failures };
}

// Quick lane: per-phase median wall and the whole compile against a
// baseline, with the compiler-speed gate's own regression budgets.
export function quickCompare(baseline, receipt, budgets) {
  const failures = [];
  const comparisons = [];
  if (baseline?.schema !== QUICK_BASELINE_SCHEMA) return { failures: ['quick-lane baseline is missing or has the wrong schema'], comparisons };
  const keys = ['cpu_model', 'threads', 'memory_bytes'];
  for (const key of keys) if (baseline.machine[key] !== receipt.machine[key]) failures.push(`baseline machine ${key} ${baseline.machine[key]} differs from ${receipt.machine[key]}`);
  if (baseline.compiler_kind !== receipt.compiler.kind) failures.push(`baseline compiler kind ${baseline.compiler_kind} differs from ${receipt.compiler.kind}`);
  for (const program of receipt.programs) {
    const base = baseline.programs[program.id];
    if (!base) { failures.push(`baseline has no ${program.id}`); continue; }
    if (base.source_sha256 !== program.source.sha256) { failures.push(`${program.id}: baseline source ${base.source_sha256} differs from ${program.source.sha256}`); continue; }
    for (const arm of program.arms) {
      const armBase = base.arms[arm.id];
      if (!armBase) { failures.push(`baseline has no ${program.id}/${arm.id}`); continue; }
      const check = (metric, current, previous, pct, floor = 0) => {
        if (previous === undefined || previous === null) return;
        const limit = previous * (1 + pct / 100);
        const gated = previous >= floor;
        const status = current === undefined ? 'missing' : !gated ? 'below-floor' : current <= limit ? 'within' : 'regressed';
        comparisons.push({ program: program.id, arm: arm.id, metric, baseline: previous, current: current ?? null, limit: Math.round(limit), status });
        if (status === 'missing' || status === 'regressed') failures.push(`${program.id}/${arm.id} ${metric}: ${current ?? 'missing'} vs baseline ${previous} (budget +${pct}%)`);
      };
      check('frontend_wall_ns', arm.summary.frontend_wall_ns.median, armBase.frontend_wall_ns, budgets.latency_regression_pct, QUICK_PHASE_FLOOR_NS);
      check('peak_rss_bytes', arm.summary.peak_rss_bytes.median, armBase.peak_rss_bytes, budgets.memory_regression_pct, QUICK_MEMORY_FLOOR_BYTES);
      for (const [kind, previous] of Object.entries(armBase.phases)) {
        check(`phase ${kind}`, arm.summary.phases[kind]?.wall_ns, previous, budgets.latency_regression_pct, QUICK_PHASE_FLOOR_NS);
      }
    }
  }
  return { failures, comparisons };
}

export function quickBaseline(receipt) {
  return {
    schema: QUICK_BASELINE_SCHEMA,
    recorded_from: { started_at: receipt.started_at, commit: receipt.commit, compiler: receipt.compiler },
    machine: receipt.machine, compiler_kind: receipt.compiler.kind,
    programs: Object.fromEntries(receipt.programs.map(program => [program.id, {
      source_sha256: program.source.sha256,
      arms: Object.fromEntries(program.arms.map(arm => [arm.id, {
        frontend_wall_ns: arm.summary.frontend_wall_ns.median,
        peak_rss_bytes: arm.summary.peak_rss_bytes.median,
        phases: Object.fromEntries(Object.entries(arm.summary.phases).map(([kind, row]) => [kind, row.wall_ns])),
      }])),
    }])),
  };
}

export async function measure(config) {
  const runId = `${new Date().toISOString().replace(/[:.]/g, '-')}-${process.pid}`;
  const root = path.join(config.workDir, runId);
  fs.mkdirSync(root, { recursive: true });
  const manifest = JSON.parse(fs.readFileSync(MANIFEST, 'utf8'));
  const cell = manifest.axes.compile_throughput;
  const receipt = {
    schema: RECEIPT_SCHEMA, lane: config.lane, started_at: new Date().toISOString(), root,
    commit: commit(), machine: machine(), contention_start: contention(),
    compiler: { kind: config.compiler.kind, path: config.compiler.path, sha256: fileSha256(config.compiler.path), build_profile: buildProfile(config.compiler, config.profile) },
    cache_lane: cell.cache_lane, samples: config.samples, warmups: config.warmups, backend: config.backend,
    jobs_control: config.singleControl ?? 'none: the compiler has no job-count control until D-JOBS1; the single arm is uncontrolled',
    programs: [], failures: [],
  };
  for (const id of config.programs) {
    const programDir = path.join(root, id);
    const prepared = id === 'selfhost' ? prepareSelfhost(config, path.join(programDir, 'prepare')) : prepareBench(id, path.join(programDir, 'prepare', 'project'));
    const programId = id === 'selfhost' && config.rung !== 'L5' ? `selfhost-${config.rung}` : id;
    const program = { id: programId, entry: prepared.entry, expected: prepared.expected, project: prepared.project ?? path.join(programDir, 'prepare', 'project'), source: prepared.source, arms: [] };
    receipt.programs.push(program);
    receipt.failures.push(...prepared.failures);
    if (prepared.failures.length) continue;
    const backendOn = config.backend && !(id === 'selfhost' && config.rung !== 'L5');
    for (const armId of config.arms) {
      const arm = { id: armId, jobs: armId === 'single' ? 1 : 'default', controlled: armId === 'default' || config.singleControl !== null, samples: [] };
      program.arms.push(arm);
      for (let index = -config.warmups; index < config.samples; index++) {
        const label = index < 0 ? `warmup${-index}` : `sample${index + 1}`;
        const dir = path.join(programDir, armId, label);
        fs.rmSync(dir, { recursive: true, force: true });
        fs.mkdirSync(dir, { recursive: true });
        process.stderr.write(`throughput: ${programId}/${armId}/${label} ...\n`);
        const compiled = await compileSample(config, program, armId, dir);
        // `jet build` already ran its backend; its artifact is always run.
        const validate = !compiled.outcome.problems.length && (backendOn || (config.compiler.kind === 'jet' && program.expected !== null));
        const backend = validate ? await backendAndRun(config, program, armId, dir, compiled) : null;
        const record = sampleRecord(compiled, backend, compiled.sample.project);
        record.label = label;
        process.stderr.write(`throughput: ${programId}/${armId}/${label}: ${(record.wall_ns / 1e9).toFixed(2)} s, ${(record.peak_rss_bytes / 2 ** 30).toFixed(2)} GiB${record.problems.length ? `, FAILED: ${record.problems.join('; ')}` : ''}\n`);
        if (index >= 0) arm.samples.push(record);
        if (record.problems.length) receipt.failures.push(`${programId}/${armId}/${label}: ${record.problems.join('; ')}`);
      }
      if (arm.samples.length === config.samples && arm.samples.every(sample => !sample.problems.length)) arm.summary = summarize(arm.samples);
      else receipt.failures.push(`${programId}/${armId}: ${config.samples - arm.samples.filter(sample => !sample.problems.length).length} of ${config.samples} samples missing or failed`);
    }
  }
  const complete = receipt.programs.filter(program => program.arms.length && program.arms.every(arm => arm.summary));
  if (config.lane === 'full') {
    const gates = cellGates(cell, complete);
    receipt.gates = gates.results;
    receipt.failures.push(...gates.failures);
  } else {
    for (const program of complete) {
      const digests = new Set(program.arms.flatMap(arm => arm.samples.map(sample => `${sample.artifact_sha256}/${sample.diagnostic_sha256}`)));
      if (digests.size !== 1) receipt.failures.push(`${program.id}: artifacts or diagnostics differ across samples and arms (${digests.size} distinct)`);
    }
    if (config.writeBaseline) {
      if (!receipt.failures.length) fs.writeFileSync(config.writeBaseline, `${JSON.stringify(quickBaseline(receipt), null, 2)}\n`);
    } else {
      const budgets = JSON.parse(fs.readFileSync(PERF_BASELINE, 'utf8')).budgets;
      const baseline = fs.existsSync(config.baseline) ? JSON.parse(fs.readFileSync(config.baseline, 'utf8')) : null;
      const compared = quickCompare(baseline, { ...receipt, programs: complete }, budgets);
      receipt.quick = { baseline: config.baseline, budgets: { latency_regression_pct: budgets.latency_regression_pct, memory_regression_pct: budgets.memory_regression_pct, time_floor_ns: QUICK_PHASE_FLOOR_NS, memory_floor_bytes: QUICK_MEMORY_FLOOR_BYTES }, comparisons: compared.comparisons };
      receipt.failures.push(...compared.failures);
    }
  }
  receipt.contention_end = contention();
  receipt.finished_at = new Date().toISOString();
  receipt.passed = receipt.failures.length === 0;
  const receiptPath = config.receipt ?? path.join(root, 'receipt.json');
  fs.mkdirSync(path.dirname(receiptPath), { recursive: true });
  fs.writeFileSync(receiptPath, `${JSON.stringify(receipt, null, 2)}\n`);
  printSummary(receipt, receiptPath);
  return receipt;
}

function printSummary(receipt, receiptPath) {
  const seconds = ns => (ns / 1e9).toFixed(2);
  for (const program of receipt.programs) {
    for (const arm of program.arms) {
      if (!arm.summary) { console.log(`${program.id}/${arm.id}: incomplete`); continue; }
      const s = arm.summary;
      console.log(`${program.id}/${arm.id}: wall ${seconds(s.wall_ns.median)} s median (${seconds(s.wall_ns.min)}..${seconds(s.wall_ns.max)}), frontend ${seconds(s.frontend_wall_ns.median)} s${s.backend_wall_ns ? `, backend ${seconds(s.backend_wall_ns.median)} s` : ''}, user ${seconds(s.cpu_user_ns.median)} s, peak RSS ${(s.peak_rss_bytes.max / 2 ** 30).toFixed(2)} GiB, threads ${s.threads_used.max}, ${Math.round(program.source.physical_lines / (s.wall_ns.median / 1e9))} lines/s`);
      for (const [kind, row] of Object.entries(s.phases)) console.log(`  ${kind.padEnd(20)} ${seconds(row.wall_ns).padStart(9)} s wall ${seconds(row.cpu_ns).padStart(9)} s cpu`);
    }
  }
  console.log(`Receipt: ${receiptPath}`);
  for (const failure of receipt.failures) console.error(`FAIL ${failure}`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  let config;
  try {
    config = options(process.argv.slice(2));
  } catch (error) {
    console.error(`${error.message}\n${USAGE}`);
    process.exit(2);
  }
  if (!config) {
    console.log(USAGE);
  } else {
    measure(config).then(receipt => { process.exitCode = receipt.passed ? 0 : 1; }, error => { console.error(error.stack ?? error.message); process.exitCode = 2; });
  }
}
