#!/usr/bin/env node
// The compiler scaling gate (Tower #4319, #4601).
//
// For every corpus shape (corpora.mjs: one stressed dimension each, or a
// realistic domain program) and every compiler, compile a size ladder (N, 2N,
// 4N, 8N by default; 125k..1M lines for domains) plus a size-1 baseline, and
// read per phase: wall time from the JET_TRACE_FILE phase spans, the
// deterministic work counters (`JET_PHASE_COUNTERS` lines: both compiler
// halves), and the trace's own span and item counts. Each series is fitted
// against k * N log N on its baseline-subtracted (marginal) values; the gate
// fails naming the shape, compiler, phase and the counter that grows fastest
// inside that phase. It also fails when a phase or the whole compile is
// slower than its lines/s floor (Tools/perf/throughput/bench.json phase
// budgets and the end-to-end floor), and projects every phase to 1M lines.
//
// Usage: node harness.mjs --compiler LABEL=KIND:PATH [--shape NAME]... [options]
// (see USAGE below). Exit 0: every series passed; 1: a growth, floor, compile
// or counter failure; 2: invalid invocation.

import fs from 'node:fs';
import { createHash } from 'node:crypto';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { LAYOUTS, SHAPES, corpus } from './corpora.mjs';
import { writeFiles } from './generate.mjs';
import { phaseTable, readTrace, runMeasured } from '../throughput/measure.mjs';
import { profileRun } from './profile.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(here, '../../..');
export const BENCH = path.join(REPO, 'Tools/perf/throughput/bench.json');
export const DOMAIN_LADDER = Object.freeze([125000, 250000, 500000, 1000000]);
export const TARGET_LINES = 1000000;
export const TARGET_SECONDS = 10;

export function parseCounters(output) {
  const counters = {};
  function walk(value, key) {
    if (typeof value === 'number') {
      if (!Number.isFinite(value) || value < 0) throw new Error(`Invalid counter: ${key}`);
      counters[key] = (counters[key] ?? 0) + value;
    } else if (value && typeof value === 'object' && !Array.isArray(value)) {
      for (const [name, child] of Object.entries(value)) walk(child, key ? `${key}.${name}` : name);
    } else {
      throw new Error(`Non-numeric counter: ${key}`);
    }
  }
  for (const line of output.split(/\r?\n/)) {
    const match = /^JET_PHASE_COUNTERS(?:\s+|:\s*)(\{.*\})\s*$/.exec(line);
    if (!match) {
      if (line.startsWith('JET_PHASE_COUNTERS')) throw new Error(`Malformed counter line: ${line}`);
      continue;
    }
    const { phase, ...values } = JSON.parse(match[1]);
    if (phase !== undefined && typeof phase !== 'string') throw new Error('Counter phase must be a string');
    walk(values, phase ?? 'total');
  }
  return Object.fromEntries(Object.entries(counters).sort(([a], [b]) => a.localeCompare(b)));
}

// ------------------------------------------------------------ growth fit

const nlogn = x => x * Math.log2(Math.max(2, x));

// Least-squares exponent of `values` against `sizes` (log-log), over the
// points with a positive value.
export function exponent(sizes, values) {
  const points = sizes.map((x, i) => [Math.log(x), values[i]]).filter(([, y]) => y > 0).map(([x, y]) => [x, Math.log(y)]);
  if (points.length < 2) return null;
  const mx = points.reduce((s, [x]) => s + x, 0) / points.length;
  const my = points.reduce((s, [, y]) => s + y, 0) / points.length;
  const sxx = points.reduce((s, [x]) => s + (x - mx) ** 2, 0);
  return sxx === 0 ? null : points.reduce((s, [x, y]) => s + (x - mx) * (y - my), 0) / sxx;
}

// Growth of a marginal series relative to N log N between its first and last
// measurable rung: 1 is N log N; > k fails. `min` is the resolution: a series
// whose last marginal value is below it is not judged, and the first rung is
// the first whose marginal reaches the resolution. A work counter
// (`exact`) that is exactly zero on every earlier rung and nonzero on the
// last "appears": work no smaller input does. Measured time or memory below
// resolution on every earlier rung is noise, not appearance.
export function growth(sizes, values, min, exact = false) {
  const usable = sizes.map((x, i) => [x, values[i]]).filter(([, v]) => v !== null && v !== undefined);
  if (usable.length < 2) return { verdict: 'unmeasured', reason: 'fewer than two rungs' };
  const [lastX, lastV] = usable[usable.length - 1];
  if (lastV < min) return { verdict: 'unmeasured', reason: `below resolution (${lastV} < ${min})` };
  const first = usable.find(([, v]) => v >= min);
  if (first[0] === lastX) {
    if (exact && usable.slice(0, -1).every(([, v]) => v === 0)) return { verdict: 'appears', reason: `zero until N=${lastX}`, ratio: Infinity };
    return { verdict: 'unmeasured', reason: `below resolution until N=${lastX}` };
  }
  const ratio = (lastV / first[1]) / (nlogn(lastX) / nlogn(first[0]));
  return { verdict: 'fitted', ratio, from: first[0], to: lastX, exponent: exponent(usable.map(([x]) => x), usable.map(([, v]) => Math.max(v, 0))) };
}

// The resolution and bound of each metric family.
export const FAMILIES = Object.freeze({
  time: { min: 0.25, bound: 1.5 },
  rss: { min: 65536, bound: 1.5 },
  counter: { min: 64, bound: 1.25 },
});
const family = key => key.startsWith('counter.') || key.startsWith('trace.') ? 'counter' : key === 'peak_rss_kb' ? 'rss' : 'time';

// Phase a metric belongs to (`phase.sema.check` -> sema.check; counters
// `counter.sema.check.scope_lookups` -> sema.check; `trace.lower.items`).
export function metricPhase(key) {
  if (key.startsWith('phase.')) return key.slice('phase.'.length);
  if (key.startsWith('counter.') || key.startsWith('trace.')) {
    const rest = key.slice(key.indexOf('.') + 1);
    return rest.slice(0, rest.lastIndexOf('.'));
  }
  return null;
}

// Judge one (shape, compiler) series. Returns per-metric growth and the
// failures, each naming the phase and its dominating counter.
export function judge(series, bounds = FAMILIES) {
  const points = series.points.filter(point => point.ok);
  const sizes = points.map(point => point.x);
  const keys = new Set(points.flatMap(point => Object.keys(point.metrics))
    .filter(key => series.mode !== 'counters' || family(key) === 'counter'));
  const metrics = {};
  for (const key of [...keys].sort()) {
    const base = series.baseline?.ok ? (series.baseline.metrics[key] ?? 0) : 0;
    const values = points.map(point => (key in point.metrics ? point.metrics[key] - base : null));
    const { min, bound } = bounds[family(key)];
    metrics[key] = { ...growth(sizes, values, min, family(key) === 'counter'), bound, values };
  }
  const failures = [];
  const superlinear = key => metrics[key].verdict === 'appears' || (metrics[key].verdict === 'fitted' && metrics[key].ratio > metrics[key].bound);
  const counterKeys = Object.keys(metrics).filter(key => key.startsWith('counter.') || key.startsWith('trace.'));
  const dominating = phase => counterKeys
    .filter(key => { const owner = metricPhase(key); return owner === phase || owner?.startsWith(`${phase}.`) || phase?.startsWith(`${owner}.`); })
    .filter(key => metrics[key].verdict === 'fitted' || metrics[key].verdict === 'appears')
    .sort((a, b) => (metrics[b].ratio ?? 0) - (metrics[a].ratio ?? 0))[0] ?? null;
  for (const key of Object.keys(metrics)) {
    if (!superlinear(key)) continue;
    const m = metrics[key];
    const phase = metricPhase(key) ?? key;
    const counter = key.startsWith('phase.') ? dominating(phase) : null;
    const counterText = key.startsWith('phase.')
      ? (counter ? `; dominating counter ${counter} x${fmt(metrics[counter].ratio)}${superlinear(counter) ? '' : ' (within bound: the phase\'s extra work is uncounted)'}` : '; no counter covers this phase')
      : '';
    failures.push({ kind: 'growth', metric: key, phase, counter, ratio: m.ratio ?? null, exponent: m.exponent ?? null,
      text: `${key} grows x${fmt(m.ratio)} faster than N log N from N=${m.from ?? '?'} to ${m.to ?? '?'} (exponent ${fmt(m.exponent)}; bound x${m.bound})${counterText}` });
  }
  return { metrics, failures };
}

const fmt = value => (typeof value === 'number' ? (Number.isFinite(value) ? value.toFixed(2) : 'inf') : '?');

// ---------------------------------------------------------- lines/s floors

// Phase floors from the throughput cell's per-phase wall budgets (seconds per
// 300k lines -> lines/s), plus the end-to-end floor of TARGET_LINES in
// TARGET_SECONDS. `startup` is an absolute budget, not a rate.
export function floors(bench = JSON.parse(fs.readFileSync(BENCH, 'utf8'))) {
  const rows = bench.phase_budgets.filter(row => row.measure === 'kinds').map(row => ({
    phase: row.phase, kinds: row.kinds, lines_per_second: 300000 / row.wall_budget_s, owners: row.owners,
  }));
  const startup = bench.phase_budgets.find(row => row.measure === 'startup');
  return { rows, startup_s: startup?.wall_budget_s ?? null, total_lines_per_second: TARGET_LINES / TARGET_SECONDS };
}

export function checkFloors(point, limits) {
  const failures = [];
  const rows = [];
  const total = point.code_lines / point.metrics.wall_seconds;
  rows.push({ phase: 'end to end', lines_per_second: total, floor: limits.total_lines_per_second });
  if (total < limits.total_lines_per_second) failures.push({ kind: 'floor', phase: 'end to end', text: `end to end ${Math.round(total)} lines/s < floor ${limits.total_lines_per_second}` });
  for (const row of limits.rows) {
    const seconds = row.kinds.reduce((sum, kind) => sum + (point.metrics[`phase.${kind}`] ?? 0), 0);
    if (!row.kinds.some(kind => `phase.${kind}` in point.metrics)) continue;
    const rate = seconds > 0 ? point.code_lines / seconds : Infinity;
    rows.push({ phase: row.phase, lines_per_second: rate, floor: row.lines_per_second, owners: row.owners });
    if (rate < row.lines_per_second) failures.push({ kind: 'floor', phase: row.phase, text: `${row.phase} (${row.kinds.join('+')}) ${Math.round(rate)} lines/s < floor ${Math.round(row.lines_per_second)} (owners ${row.owners.join(', ')})` });
  }
  if (limits.startup_s !== null && point.metrics.startup_seconds !== undefined && point.metrics.startup_seconds > limits.startup_s) {
    failures.push({ kind: 'floor', phase: 'startup', text: `startup ${point.metrics.startup_seconds.toFixed(3)} s > ${limits.startup_s} s` });
  }
  return { rows, failures };
}

// Every phase's wall at TARGET_LINES from the largest measured rung and the
// fitted exponent (never below 1): a projection, not a measurement.
export function project(series, judged) {
  const points = series.points.filter(point => point.ok);
  if (!points.length) return null;
  const top = points[points.length - 1];
  const scale = TARGET_LINES / top.code_lines;
  const phases = {};
  for (const [key, value] of Object.entries(top.metrics)) {
    if (!key.startsWith('phase.') && key !== 'wall_seconds') continue;
    const b = Math.max(1, judged.metrics[key]?.exponent ?? 1);
    phases[key] = { measured_s: value, at_lines: top.code_lines, exponent: b, projected_s: value * scale ** b };
  }
  return { measured_at_lines: top.code_lines, target_lines: TARGET_LINES, target_seconds: TARGET_SECONDS, phases };
}

// ------------------------------------------------------------- compilers

export function parseCompiler(spec) {
  const match = /^([\w.-]+)=(jet|bootstrap|command):(.+)$/.exec(spec);
  if (!match) throw new Error(`--compiler expects LABEL=jet:PATH, LABEL=bootstrap:PATH or LABEL=command:TEMPLATE (got ${spec})`);
  const [, label, kind, target] = match;
  return { label, kind, path: kind === 'command' ? null : path.resolve(target.replace(/^~(?=\/)/, os.homedir())), template: kind === 'command' ? target : null };
}

const quote = value => `'${String(value).replaceAll("'", "'\\''")}'`;

function argvFor(config, compiler, run) {
  const scope = argv => ['systemd-run', '--user', '--scope', '-q', '-p', `MemoryMax=${config.memoryMax}`, '-p', 'MemorySwapMax=0',
    'bash', '-c', 'ulimit -c 0; ulimit -s 1048576; exec "$@"', 'scoped', ...argv];
  const common = { JET_STORE_DIR: run.store, JET_TRACE_FILE: run.trace, JET_PHASE_COUNTERS: '1', TMPDIR: run.dir, NO_COLOR: '1' };
  if (compiler.kind === 'bootstrap') {
    const env = { HOME: os.homedir(), PATH: process.env.PATH, LC_ALL: 'C', JET_BOOTSTRAP_MODE: 'runner', JET_BOOTSTRAP_FACTORY_TIER: 'aot',
      JET_BOOTSTRAP_SOURCE_ROOT: run.project, JET_BOOTSTRAP_ENTRY: 'main.jet', JET_BOOTSTRAP_OUTPUT: path.join(run.dir, 'out.rs'),
      JET_BOOTSTRAP_RECEIPT: run.receipt, RUST_MIN_STACK: '8388608', ...common };
    return { argv: scope(['env', '-i', ...Object.entries(env).map(([k, v]) => `${k}=${v}`), compiler.path]), env: process.env };
  }
  if (compiler.kind === 'jet') {
    return { argv: scope([compiler.path, config.jetCommand, '--color=never', path.join(run.project, 'main.jet')]), env: { ...process.env, ...common } };
  }
  const command = compiler.template.replace(/\{([a-z]+)\}/g, (_, key) => {
    const values = { project: run.project, entry: path.join(run.project, 'main.jet'), n: run.n, trace: run.trace, dir: run.dir };
    if (!(key in values)) throw new Error(`Unknown command placeholder: {${key}}`);
    return quote(values[key]);
  });
  return { argv: ['/bin/sh', '-c', command], env: { ...process.env, ...common } };
}

// Diagnostics a compile reported: the bootstrap receipt's report rows, or
// `Error [CODE]` lines of `jet`.
function outcome(compiler, run, code, text) {
  if (compiler.kind === 'bootstrap') {
    const receipt = fs.existsSync(run.receipt) ? fs.readFileSync(run.receipt, 'utf8') : '';
    const errors = receipt.split('\n').filter(line => line.startsWith('report_json=')).map(line => { try { return JSON.parse(line.slice(12)); } catch { return {}; } })
      .filter(report => !String(report.code ?? '').startsWith('L')).length;
    return { complete: /^complete=true$/m.test(receipt), errors };
  }
  return { complete: code === 0, errors: (text.match(/^Error \[E/gm) ?? []).length };
}

// One measured compile of one corpus.
async function measure(config, compiler, built, dir) {
  fs.rmSync(dir, { recursive: true, force: true });
  fs.mkdirSync(dir, { recursive: true });
  // Keep only receipts/traces/logs in the evidence tree. Projects, stores,
  // emitted Rust, executable outputs and perf.data live on the scratch drive.
  const buildDir = path.join(config.buildRoot, createHash('sha256').update(path.resolve(dir)).digest('hex'));
  fs.rmSync(buildDir, { recursive: true, force: true });
  const run = { dir: buildDir, project: path.join(buildDir, 'project'), store: path.join(buildDir, 'store'),
    trace: path.join(dir, 'trace.json'), receipt: path.join(dir, 'receipt'), n: built.n };
  writeFiles(built.files, run.project);
  fs.mkdirSync(run.store, { recursive: true });
  const { argv, env } = argvFor(config, compiler, run);
  const stdoutPath = path.join(dir, 'stdout.log');
  const stderrPath = path.join(dir, 'stderr.log');
  const measured = await runMeasured(config.profile ? [...config.profile.wrap(buildDir), ...argv] : argv,
    { cwd: run.project, env, stdoutPath, stderrPath, timeoutMs: config.timeoutS * 1000 });
  const text = `${fs.readFileSync(stdoutPath, 'utf8')}\n${fs.readFileSync(stderrPath, 'utf8')}`;
  const problems = [];
  let counters = {};
  try { counters = parseCounters(text); } catch (error) { problems.push(error.message); }
  const events = fs.existsSync(run.trace) ? readTrace(run.trace) : [];
  const start = events.find(event => event.name === 'trace.start');
  const offsetNs = start ? (start.args.unix_ms - measured.spawnUnixMs) * 1e6 : 0;
  const { phases, firstBeginNs, unclosed } = phaseTable(events, measured.samples, offsetNs);
  const result = outcome(compiler, run, measured.code, text);
  const metrics = {
    wall_seconds: measured.wallNs / 1e9, user_seconds: (measured.rusage?.userNs ?? 0) / 1e9, peak_rss_kb: Math.round(measured.peakRssBytes / 1024),
  };
  if (firstBeginNs !== null) metrics.startup_seconds = firstBeginNs / 1e9;
  for (const [kind, phase] of Object.entries(phases)) {
    metrics[`phase.${kind}`] = phase.wall_ns / 1e9;
    metrics[`trace.${kind}.spans`] = phase.count;
    if (phase.items !== null) metrics[`trace.${kind}.items`] = phase.items;
  }
  for (const [name, value] of Object.entries(counters)) metrics[`counter.${name}`] = value;
  const open = [...new Set(unclosed)];
  const timeoutPhase = measured.killed ? open[open.length - 1] ?? (events.length ? 'between phases' : 'startup') : null;
  if (measured.killed) problems.push(`timed out after ${config.timeoutS} s in ${timeoutPhase}`);
  if (!events.length) problems.push('no JET_TRACE_FILE trace');
  const expectErrors = built.expect.diagnostics;
  if (expectErrors === 0 && (!result.complete || result.errors)) problems.push(`compile failed (exit ${measured.code}${measured.signal ? `, ${measured.signal}` : ''}, ${result.errors} errors); see ${dir}`);
  if (expectErrors > 0 && result.errors < expectErrors) problems.push(`reported ${result.errors} of ${expectErrors} expected diagnostics; see ${dir}`);
  metrics.diagnostics = result.errors;
  return { n: built.n, x: built.kind === 'domain' ? built.code_lines : built.n, code_lines: built.code_lines, source_sha256: built.source_sha256,
    ok: problems.length === 0, problems, open_phases: open, timeout_phase: timeoutPhase, metrics, dir, build_dir: buildDir, exit: measured.code };
}

function ladderFor(config, shape) {
  if (config.ladders[shape]) return config.ladders[shape];
  const entry = SHAPES[shape];
  if (entry.kind === 'domain') return config.domainLadder;
  const base = config.bases[shape] ?? entry.base;
  return Array.from({ length: config.steps }, (_, i) => base * 2 ** i);
}

// A receipt is a checkpoint, not a cache across different compiler invocations.
export function resumeReceipt(config, limits, previous = null) {
  const invocation = JSON.stringify({
    compilers: config.compilers, shapes: config.shapes, layouts: config.layouts,
    ladders: Object.fromEntries(config.shapes.map(shape => [shape, ladderFor(config, shape)])),
    seed: config.seed, memoryMax: config.memoryMax, timeoutS: config.timeoutS,
    failurePolicy: 'all-rungs',
    stopAfterS: config.stopAfterS, profileTop: config.profileTop, buildRoot: config.buildRoot, mode: config.mode, jobs: config.jobs, families: FAMILIES, floors: limits,
  });
  if (previous) {
    if (previous.invocation !== invocation) throw new Error('Receipt invocation differs; use a new --work-dir');
    return previous;
  }
  return {
    schema: 'jet.scaling-gate/v3', invocation, startedAt: new Date().toISOString(), workDir: config.workDir, host: os.hostname(),
    compilers: config.compilers, families: FAMILIES, floors: limits, target: { lines: TARGET_LINES, seconds: TARGET_SECONDS },
    memoryMax: config.memoryMax, timeoutS: config.timeoutS, stopAfterS: config.stopAfterS, buildRoot: config.buildRoot, mode: config.mode, jobs: config.jobs, series: [], failures: [],
  };
}

// Bounded parallelism for counter-only compiles. Wall-clock verdicts remain serial.
export async function concurrent(values, jobs, visit) {
  let position = 0;
  await Promise.all(Array.from({ length: Math.min(jobs, values.length) }, async () => {
    while (position < values.length) {
      const value = values[position++];
      await visit(value);
    }
  }));
}

export async function run(config) {
  fs.mkdirSync(config.workDir, { recursive: true });
  const limits = floors();
  const receipt = resumeReceipt(config, limits, fs.existsSync(config.receipt) ? JSON.parse(fs.readFileSync(config.receipt, 'utf8')) : null);
  const save = () => {
    const pending = `${config.receipt}.pending`;
    fs.writeFileSync(pending, `${JSON.stringify(receipt, null, 2)}\n`);
    fs.renameSync(pending, config.receipt);
  };
  save();
  for (const shape of config.shapes) {
    for (const compiler of config.compilers) {
    for (const layout of config.layouts) {
      const ladder = ladderFor(config, shape);
      const label = `${shape}[${layout}]/${compiler.label}`;
      let series = receipt.series.find(s => s.shape === shape && s.compiler === compiler.label && s.layout === layout);
      if (series?.done) continue;
      if (!series) {
        series = { shape, layout, compiler: compiler.label, mode: config.mode, kind: SHAPES[shape].kind, dimension: SHAPES[shape].dimension, ladder, baseline: null, points: [], truncated: null };
        receipt.series.push(series);
      }
      const where = n => path.join(config.workDir, compiler.label, `${shape}-${layout}`, String(n));
      if (config.mode === 'counters') {
        await concurrent([1, ...ladder], config.jobs, async n => {
          if (n === 1 ? series.baseline : series.points.some(point => point.n === n)) return;
          const point = await measure(config, compiler, corpus(shape, n, config.seed, layout), where(n));
          if (n === 1) series.baseline = point;
          else {
            series.points.push(point);
            series.points.sort((a, b) => ladder.indexOf(a.n) - ladder.indexOf(b.n));
          }
          save();
          log(config, `${label} N=${n} ${summary(point)} (counter-only; time/RSS not gated)`);
        });
      } else {
      if (!series.baseline) {
        series.baseline = await measure(config, compiler, corpus(shape, 1, config.seed, layout), where(1));
        save();
        log(config, `${label} N=1 ${summary(series.baseline)}`);
      }
      for (const n of ladder) {
        let point = series.points.find(point => point.n === n);
        if (!point) {
          const built = corpus(shape, n, config.seed, layout);
          point = await measure(config, compiler, built, where(n));
          series.points.push(point);
          save();
          log(config, `${label} N=${n} (${built.code_lines} lines) ${summary(point)}`);
        }
        if (point.metrics.wall_seconds > config.stopAfterS && n !== ladder[ladder.length - 1]) {
          series.truncated = `stopped after N=${n}: ${point.metrics.wall_seconds.toFixed(1)} s > --stop-after ${config.stopAfterS} s`;
          break;
        }
      }
      }
      const judged = judge(series);
      series.metrics = judged.metrics;
      series.failures = judged.failures;
      if (!series.baseline.ok) series.failures.push({ kind: series.baseline.timeout_phase ? 'timeout' : 'compile',
        phase: series.baseline.timeout_phase ?? series.baseline.open_phases?.at(-1) ?? null, text: `baseline: ${series.baseline.problems.join('; ')}` });
      const ok = series.points.filter(point => point.ok);
      if (config.mode !== 'counters' && ok.length) {
        const checked = checkFloors(ok[ok.length - 1], limits);
        series.floors = checked.rows;
        series.failures.push(...checked.failures);
      }
      if (series.truncated) {
        series.failures.push({ kind: 'truncated', phase: null, text: series.truncated });
      }
      for (const failed of series.points.filter(point => !point.ok)) {
        series.failures.push({ kind: failed.timeout_phase ? 'timeout' : 'compile', phase: failed.timeout_phase ?? failed.open_phases?.at(-1) ?? null,
          text: `N=${failed.n}: ${failed.problems.join('; ')}` });
      }
      if (config.mode !== 'counters' && SHAPES[shape].kind === 'domain') series.projection = project(series, judged);
      save();
      if (config.mode !== 'counters' && config.profileTop && series.failures.some(failure => failure.kind === 'growth') && ok.length && !series.profile) {
        series.profile = await profileTop(config, compiler, shape, layout, ok[ok.length - 1], series, save);
      }
      series.done = true;
      save();
    }
    }
  }
  receipt.failures = receipt.series.flatMap(series => (series.failures ?? []).map(failure =>
    `${series.shape}[${series.layout}]/${series.compiler}: ${failure.text}`));
  receipt.passed = receipt.failures.length === 0;
  receipt.finishedAt = new Date().toISOString();
  save();
  return receipt;
}

// Re-run the largest passing rung under perf and attribute samples to the
// failing phases (profile.mjs): the responsible functions with their source.
async function profileTop(config, compiler, shape, layout, point, series, save) {
  const dir = path.join(config.workDir, compiler.label, `${shape}-${layout}`, `${point.n}-profile`);
  const phases = [...new Set(series.failures.filter(f => f.kind === 'growth').map(f => f.phase))];
  if (!series.profilePoint) {
    series.profilePoint = await measure({ ...config, profile: { wrap: d => profileRun.wrap(d) } }, compiler, corpus(shape, point.n, config.seed, layout), dir);
    save();
  }
  return profileRun.attribute(series.profilePoint.build_dir, phases, { repo: REPO, top: 12, ok: series.profilePoint.ok, traceFile: path.join(dir, 'trace.json') });
}

function summary(point) {
  const phases = Object.entries(point.metrics).filter(([k]) => k.startsWith('phase.')).sort(([, a], [, b]) => b - a).slice(0, 3)
    .map(([k, v]) => `${k.slice(6)} ${v.toFixed(2)}s`).join(', ');
  return `${point.ok ? 'ok' : 'FAILED'} wall ${point.metrics.wall_seconds.toFixed(2)}s rss ${(point.metrics.peak_rss_kb / 1048576).toFixed(2)}G [${phases}]${point.ok ? '' : ` ${point.problems.join('; ')}`}`;
}

function log(config, line) {
  console.log(line);
  fs.appendFileSync(path.join(config.workDir, 'gate.log'), `${new Date().toISOString()} ${line}\n`);
}

const USAGE = `Usage: node harness.mjs --compiler LABEL=KIND:PATH [--compiler ...] [options]
  KIND: jet (\`jet build\`; --jet-command check), bootstrap (a generated compiler, runner protocol),
        command (a shell template: {project} {entry} {n} {trace} {dir}).
  --shape NAME           repeatable; default every shape (node generate.mjs --list)
  --stress | --domains   every stress shape / every domain shape
  --base SHAPE=N         stress ladder base (ladder N, 2N, 4N, 8N); --steps K for K rungs
  --ladder SHAPE=A,B,..  explicit ladder for one shape
  --domain-ladder A,B,.. domain ladder in code lines (default ${DOMAIN_LADDER.join(',')})
  --memory-max 20G  --timeout 1800  --stop-after SECONDS (skip larger rungs after a slower one)
  --profile              perf-profile the top rung of every failing series (profile.mjs)
  --layout files|single  repeatable; default both (a multi-file failure is a gate failure)
  --work-dir DIR  --receipt FILE  --seed S
  --build-root DIR       projects/stores/compiler outputs/perf.data (default /mnt/jetscratch/scaling-gate)
  --mode timing|counters counter-only mode gates deterministic work, not time/RSS/floors
  --jobs N              parallel counter-mode compiles (default 1); timing stays serial
Receipts checkpoint every baseline, rung and profile compile; rerunning the same
invocation resumes without retrying completed compiles, including timeouts.
Exit 0 pass, 1 any growth/floor/compile failure, 2 invalid invocation.`;

export function options(args) {
  const config = { compilers: [], shapes: [], bases: {}, ladders: {}, steps: 4, domainLadder: [...DOMAIN_LADDER], memoryMax: '20G', timeoutS: 1800,
    stopAfterS: Infinity, jetCommand: 'build', seed: 1, profileTop: false, layouts: [], buildRoot: '/mnt/jetscratch/scaling-gate', mode: 'timing', jobs: 1,
    workDir: path.join(os.homedir(), '.cache/jet-dev/scratch/scaling-gate', new Date().toISOString().replace(/[:.]/g, '-')) };
  for (let i = 0; i < args.length; i++) {
    const flag = args[i];
    if (flag === '--help') return null;
    if (flag === '--stress') { config.shapes.push(...Object.keys(SHAPES).filter(s => SHAPES[s].kind === 'stress')); continue; }
    if (flag === '--domains') { config.shapes.push(...Object.keys(SHAPES).filter(s => SHAPES[s].kind === 'domain')); continue; }
    if (flag === '--profile') { config.profileTop = true; continue; }
    const value = args[++i];
    if (value === undefined) throw new Error(`Missing value for ${flag}`);
    const sizes = text => text.split(',').map(Number).filter(n => Number.isSafeInteger(n) && n > 1);
    switch (flag) {
      case '--compiler': config.compilers.push(parseCompiler(value)); break;
      case '--shape': if (!SHAPES[value]) throw new Error(`Unknown shape ${value}`); config.shapes.push(value); break;
      case '--base': { const [shape, n] = value.split('='); if (!SHAPES[shape] || !(Number(n) > 1)) throw new Error(`--base expects SHAPE=N`); config.bases[shape] = Number(n); break; }
      case '--ladder': { const [shape, list] = value.split('='); if (!SHAPES[shape] || sizes(list ?? '').length < 2) throw new Error('--ladder expects SHAPE=A,B,...'); config.ladders[shape] = sizes(list); break; }
      case '--domain-ladder': config.domainLadder = sizes(value); if (config.domainLadder.length < 2) throw new Error('--domain-ladder needs two sizes'); break;
      case '--steps': config.steps = Number(value); if (!(config.steps >= 2)) throw new Error('--steps must be >= 2'); break;
      case '--memory-max': config.memoryMax = value; break;
      case '--timeout': config.timeoutS = Number(value); break;
      case '--stop-after': config.stopAfterS = Number(value); break;
      case '--jet-command': if (!['build', 'check'].includes(value)) throw new Error('--jet-command is build or check'); config.jetCommand = value; break;
      case '--work-dir': config.workDir = path.resolve(value); break;
      case '--build-root': config.buildRoot = path.resolve(value); break;
      case '--receipt': config.receipt = path.resolve(value); break;
      case '--layout': if (!LAYOUTS.includes(value)) throw new Error(`--layout is ${LAYOUTS.join(' or ')}`); config.layouts.push(value); break;
      case '--seed': config.seed = Number(value); break;
      case '--mode': if (!['timing', 'counters'].includes(value)) throw new Error('--mode is timing or counters'); config.mode = value; break;
      case '--jobs': config.jobs = Number(value); if (!Number.isSafeInteger(config.jobs) || config.jobs < 1) throw new Error('--jobs must be a positive integer'); break;
      default: throw new Error(`Unknown option: ${flag}`);
    }
  }
  if (!config.compilers.length) throw new Error('At least one --compiler is required');
  if (new Set(config.compilers.map(c => c.label)).size !== config.compilers.length) throw new Error('Compiler labels must be unique');
  if (!config.shapes.length) config.shapes = Object.keys(SHAPES);
  config.shapes = [...new Set(config.shapes)];
  config.layouts = config.layouts.length ? [...new Set(config.layouts)] : [...LAYOUTS];
  config.receipt ??= path.join(config.workDir, 'receipt.json');
  return config;
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
    const receipt = await run(config);
    console.log(`Receipt: ${config.receipt}`);
    for (const failure of receipt.failures) console.error(`FAIL ${failure}`);
    process.exitCode = receipt.passed ? 0 : 1;
  }
}
