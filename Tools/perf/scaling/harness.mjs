#!/usr/bin/env node
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { AXES, generate, sources } from './generate.mjs';

export const DEFAULT_JET = path.join(os.homedir(), '.cache/jet-test-scratch/jet-release-night12/jet');
export const GNU_TIME = '/nix/store/n0wrh3vjfwcqfyswwai0zcxvkpibq34v-time-1.10/bin/time';
// No exemptions currently. An exemption must name an axis and explain its bound.
export const EXEMPTIONS = Object.freeze({});
// Modules already exceed the memory bound at 128 files; keep their ladder
// within an 8 GiB lane. Depth and flat match tables hit Jet's frame limit.
export const DEFAULT_BASES = Object.freeze({
  functions: 256, declarations: 256, depth: 16, modules: 64,
  'match-arms': 48, 'string-literals': 1048576, interpolation: 256,
  'fan-in': 256, generics: 256,
});
const quote = value => `'${String(value).replaceAll("'", "'\\''")}'`;

export function render(template, values) {
  return template.replace(/\{([a-z]+)\}/g, (_, key) => {
    if (!(key in values)) throw new Error(`Unknown command placeholder: {${key}}`);
    return quote(values[key]);
  });
}

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

export function validateOverrides(overrides) {
  if (!overrides || typeof overrides !== 'object' || Array.isArray(overrides)) throw new Error('Overrides must be an axis table');
  for (const [axis, exemption] of Object.entries(overrides)) {
    if (!AXES.includes(axis) || !exemption || !Number.isFinite(exemption.bound) || exemption.bound < 1
      || typeof exemption.reason !== 'string' || !exemption.reason.trim()) {
      throw new Error(`Invalid exemption for ${axis}: require a known axis, bound >= 1, and reason`);
    }
  }
  return overrides;
}

const median = values => [...values].sort((a, b) => a - b)[Math.floor(values.length / 2)];
export function growth(lower, upper) {
  return lower === 0 ? (upper === 0 ? 1 : 'Infinity') : upper / lower;
}

export function compare(points, bound, baseline) {
  const ratios = [];
  const failures = [];
  for (let i = 1; i < points.length; i++) {
    const previous = points[i - 1];
    const current = points[i];
    const metrics = new Set([...Object.keys(baseline.metrics), ...Object.keys(previous.metrics), ...Object.keys(current.metrics)]);
    const values = {};
    for (const metric of metrics) {
      if (!(metric in baseline.metrics) || !(metric in previous.metrics) || !(metric in current.metrics)) {
        failures.push(`${metric}: missing metric at baseline, N=${previous.n}, or ${current.n}`);
        values[metric] = null;
        continue;
      }
      // GNU time %e/%U have centisecond precision. Subtract integer units,
      // so an exact 2.2x boundary cannot become 2.2000000000000055x.
      const seconds = metric === 'wall_seconds' || metric === 'user_seconds';
      const units = value => seconds ? Math.round(value * 100) : value;
      const lower = units(previous.metrics[metric]) - units(baseline.metrics[metric]);
      const upper = units(current.metrics[metric]) - units(baseline.metrics[metric]);
      if (lower <= 0 || upper < 0) {
        values[metric] = null;
        failures.push(`${metric}: non-positive marginal baseline at ${previous.n}->${current.n} (${lower / (seconds ? 100 : 1)}, ${upper / (seconds ? 100 : 1)}); growth is not measurable`);
        continue;
      }
      const ratio = growth(lower, upper);
      values[metric] = ratio;
      if (ratio === 'Infinity' || ratio > bound) failures.push(`${metric}: marginal ${previous.n}->${current.n} grew ${ratio}x > ${bound}x`);
    }
    ratios.push({ from: previous.n, to: current.n, metrics: values });
  }
  return { ratios, failures };
}

function options(args) {
  const result = { n: null, repeats: 3, threshold: 2.2, axes: [], commands: [], jet: DEFAULT_JET, time: GNU_TIME, overrides: EXEMPTIONS };
  for (let i = 0; i < args.length; i++) {
    const flag = args[i];
    if (flag === '--help') return null;
    const value = args[++i];
    if (value === undefined) throw new Error(`Missing value for ${flag}`);
    switch (flag) {
      case '--n': result.n = Number(value); break;
      case '--repeats': result.repeats = Number(value); break;
      case '--threshold': result.threshold = Number(value); break;
      case '--axis': result.axes.push(value); break;
      case '--jet': result.jet = path.resolve(value); break;
      case '--time': result.time = path.resolve(value); break;
      case '--work-dir': result.workDir = path.resolve(value); break;
      case '--receipt': result.receipt = path.resolve(value); break;
      case '--overrides': result.overrides = validateOverrides(JSON.parse(fs.readFileSync(value, 'utf8'))); break;
      case '--command': {
        const split = value.indexOf('=');
        if (split < 1 || !value.slice(split + 1).trim()) throw new Error('--command expects LABEL=TEMPLATE');
        result.commands.push({ label: value.slice(0, split), template: value.slice(split + 1) });
        break;
      }
      default: throw new Error(`Unknown option: ${flag}`);
    }
  }
  if (result.n !== null && (!Number.isSafeInteger(result.n) || result.n < 2 || !Number.isSafeInteger(result.n * 4))) throw new Error('N must be >= 2 with safe 4N');
  if (!Number.isSafeInteger(result.repeats) || result.repeats < 1) throw new Error('Repeats must be a positive integer');
  if (!Number.isFinite(result.threshold) || result.threshold < 1) throw new Error('Threshold must be finite and >= 1');
  if (!result.axes.length) result.axes = [...AXES];
  if (new Set(result.axes).size !== result.axes.length || result.axes.some(axis => !AXES.includes(axis))) throw new Error('Axes must be unique known names');
  if (!result.commands.length) result.commands = [{ label: 'check', template: '{jet} check {source}' }, { label: 'build', template: '{jet} build {source}' }];
  if (new Set(result.commands.map(command => command.label)).size !== result.commands.length) throw new Error('Command labels must be unique');
  return result;
}

export function run(config) {
  const parent = config.workDir ?? os.tmpdir();
  fs.mkdirSync(parent, { recursive: true });
  const root = fs.mkdtempSync(path.join(parent, 'jet-scaling-'));
  const receiptPath = config.receipt ?? path.join(root, 'receipt.json');
  const receipt = {
    schema: 2, startedAt: new Date().toISOString(), root,
    bases: Object.fromEntries(config.axes.map(axis => [axis, config.n ?? DEFAULT_BASES[axis]])),
    baselineN: 1, ratioMode: '(metric(upper) - metric(1)) / (metric(lower) - metric(1))',
    repeats: config.repeats, threshold: config.threshold,
    exemptions: config.overrides, commands: config.commands, time: config.time,
    environment: { platform: process.platform, arch: process.arch, node: process.version },
    metrics: { wall_seconds: 'GNU time %e', user_seconds: 'GNU time %U', peak_rss_kib: 'GNU time %M' },
    results: [], failures: [],
  };
  for (const axis of config.axes) {
    for (const [commandIndex, compiler] of config.commands.entries()) {
      const points = [];
      const base = config.n ?? DEFAULT_BASES[axis];
      const result = { axis, compiler: compiler.label, base, bound: config.overrides[axis]?.bound ?? config.threshold,
        reason: config.overrides[axis]?.reason ?? null, baseline: null, points };
      receipt.results.push(result);
      try {
        for (const n of [1, base, base * 2, base * 4]) {
          const samples = [];
          const sourceHash = crypto.createHash('sha256').update(JSON.stringify(sources(axis, n))).digest('hex');
          const point = { n, sourceHash, samples, metrics: {} };
          if (n === 1) result.baseline = point;
          else points.push(point);
          for (let repeat = 0; repeat < config.repeats; repeat++) {
            // Fresh source and build directory per timed process: no incremental
            // cache reuse, and generation is outside the timed interval.
            const directory = path.join(root, axis, `command-${commandIndex}`, `${n}-${repeat}`);
            const source = generate(axis, n, directory);
            const command = render(compiler.template, { jet: config.jet, source, dir: directory,
              out: path.join(directory, 'program'), axis, n });
            const timing = path.join(directory, 'time.txt');
            const child = spawnSync(config.time, ['-f', '%e\t%U\t%M', '-o', timing, '/bin/sh', '-c', command],
              { cwd: directory, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
            fs.writeFileSync(path.join(directory, 'stdout.log'), child.stdout ?? '');
            fs.writeFileSync(path.join(directory, 'stderr.log'), child.stderr ?? '');
            const sample = { directory, command, exitCode: child.status, signal: child.signal };
            samples.push(sample);
            if (child.error || child.status !== 0) throw new Error(`N=${n}, repeat=${repeat}: ${child.error?.message ?? `exit ${child.status}, signal ${child.signal}`}; see ${directory}/stderr.log`);
            const timingText = fs.readFileSync(timing, 'utf8').trim();
            if (!/^\d+(?:\.\d+)?\t\d+(?:\.\d+)?\t\d+$/.test(timingText)) throw new Error(`Invalid GNU time output: ${timingText}`);
            const [wall, user, rss] = timingText.split('\t').map(Number);
            sample.metrics = { wall_seconds: wall, user_seconds: user, peak_rss_kib: rss };
            sample.counters = parseCounters(`${child.stdout}\n${child.stderr}`);
            for (const [name, value] of Object.entries(sample.counters)) sample.metrics[`counter.${name}`] = value;
          }
          if (samples.some(sample => JSON.stringify(sample.counters) !== JSON.stringify(samples[0].counters))) throw new Error(`N=${n}: counters differ between repeated identical inputs`);
          for (const key of Object.keys(samples[0].metrics)) point.metrics[key] = median(samples.map(sample => sample.metrics[key]));
        }
        Object.assign(result, compare(points, result.bound, result.baseline));
        for (const failure of result.failures) receipt.failures.push(`${axis}/${compiler.label}: ${failure}`);
      } catch (error) {
        result.error = error.message;
        receipt.failures.push(`${axis}/${compiler.label}: ${error.message}`);
      }
    }
  }
  receipt.passed = receipt.failures.length === 0;
  receipt.finishedAt = new Date().toISOString();
  fs.mkdirSync(path.dirname(receiptPath), { recursive: true });
  fs.writeFileSync(receiptPath, `${JSON.stringify(receipt, null, 2)}\n`);
  console.log('axis              compiler     marginal wall       marginal user       marginal RSS        gate');
  const format = value => typeof value === 'number' ? value.toFixed(2) : (value ?? '?');
  for (const result of receipt.results) {
    const pair = key => result.ratios?.map(ratio => format(ratio.metrics[key])).join('/') ?? '-';
    const failed = result.error || result.failures?.length;
    console.log(`${result.axis.padEnd(18)}${result.compiler.padEnd(13)}${pair('wall_seconds').padEnd(20)}${pair('user_seconds').padEnd(20)}${pair('peak_rss_kib').padEnd(19)}${failed ? 'FAIL' : 'PASS'}`);
  }
  console.log(`Receipt: ${receiptPath}`);
  for (const failure of receipt.failures) console.error(failure);
  return receipt;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const config = options(process.argv.slice(2));
    if (!config) {
      console.log('Usage: node harness.mjs [--n BASE] [--repeats 3] [--axis AXIS] [--threshold 2.2]\n  [--jet PATH] [--time PATH] [--work-dir DIR] [--receipt FILE]\n  [--command LABEL=TEMPLATE] [--overrides FILE]\nPlaceholders: {jet}, {source}, {dir}, {out}, {axis}, {n}; each is shell-quoted.\nDefault commands: jet check, jet build. Default axis ladders: 256/512/1024, except modules 64/128/256, depth 16/32/64, match-arms 48/96/192, string-literals 1048576/2097152/4194304.\nEvery ladder also measures N=1; ratios use baseline-subtracted marginals.');
    } else {
      process.exitCode = run(config).passed ? 0 : 1;
    }
  } catch (error) {
    console.error(error.message);
    process.exitCode = 2;
  }
}
