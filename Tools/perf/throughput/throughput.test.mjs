import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { mixedSources, countLines } from '../scaling/mixed.mjs';
import { cellGates, PINS, phaseBudgetCheck, quickCompare } from './harness.mjs';
import { cpuAt, phaseTable } from './measure.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const scratch = process.env.JET_THROUGHPUT_SCRATCH ?? path.join(os.homedir(), '.cache/jet-dev/scratch/throughput-test');
fs.mkdirSync(scratch, { recursive: true });
const root = fs.mkdtempSync(path.join(scratch, 'test-'));

test('mixed is deterministic, sized by code lines, and pinned', () => {
  const a = mixedSources(2000, 7);
  assert.deepEqual(a, mixedSources(2000, 7));
  assert.notEqual(a.source_sha256, mixedSources(2000, 8).source_sha256);
  assert.ok(a.stats.code_lines >= 2000);
  assert.equal(countLines(a.files['main.jet']).code, a.stats.code_lines);
  assert.match(a.expected, /^mixed checksum \d+ sections \d+\n$/);
  assert.match(a.files['main.jet'], /prep \{/);
  assert.match(a.files['main.jet'], /fn pick_m0<T>/);
  assert.match(a.files['main.jet'], /\.map\(\(value\) ->/);
  for (const [id, pin] of Object.entries(PINS.programs)) {
    const generated = mixedSources(pin.code_lines, pin.seed);
    assert.equal(generated.source_sha256, pin.source_sha256, id);
    assert.equal(generated.expected, pin.expected_output, id);
    assert.deepEqual(Object.fromEntries(Object.keys(pin.shape).map(key => [key, generated.stats[key]])), pin.shape, id);
  }
  assert.throws(() => mixedSources(0, 1), /positive/);
});

test('phase table unions spans per kind and reads CPU across them', () => {
  const samples = [{ tNs: 0, cpuNs: 0 }, { tNs: 1e9, cpuNs: 1e9 }, { tNs: 2e9, cpuNs: 3e9 }];
  assert.equal(cpuAt(samples, 1.5e9), 2e9);
  const events = [
    { name: 'load', cat: 'load', ph: 'B', ts: 0, tid: 1 },
    { name: 'load', cat: 'load', ph: 'E', ts: 500000, tid: 1, args: { items: 2 } },
    { name: 'lower', cat: 'lower', ph: 'B', ts: 1000000, tid: 1 },
    { name: 'lower', cat: 'lower', ph: 'B', ts: 1200000, tid: 2 },
    { name: 'f', cat: 'lower.function', ph: 'B', ts: 1300000, tid: 2 },
    { name: 'f', cat: 'lower.function', ph: 'E', ts: 1400000, tid: 2 },
    { name: 'lower', cat: 'lower', ph: 'E', ts: 1600000, tid: 2 },
    { name: 'lower', cat: 'lower', ph: 'E', ts: 2000000, tid: 1 },
    { name: 'emit', cat: 'emit', ph: 'B', ts: 2000000, tid: 1 },
  ];
  const { phases, unclosed, firstBeginNs } = phaseTable(events, samples, 0);
  assert.equal(firstBeginNs, 0);
  assert.deepEqual(unclosed, ['emit']);
  assert.deepEqual(phases.load, { wall_ns: 5e8, span_ns: 5e8, cpu_ns: 5e8, count: 1, items: 2 });
  assert.deepEqual(phases.lower, { wall_ns: 1e9, span_ns: 1.4e9, cpu_ns: 2e9, count: 2, items: null });
  assert.equal(phases['lower.function'], undefined);
});

test('budgets, gates and the quick comparison fail closed', () => {
  const sample = (wall, digest) => ({ wall_ns: wall, artifact_sha256: digest, diagnostic_sha256: 'd', frontend: { startup_ns: 1 } });
  const summary = (wall, rss) => ({ wall_ns: { median: wall, min: wall, max: wall }, peak_rss_bytes: { median: rss, max: rss }, frontend_wall_ns: { median: wall }, phases: { load: { wall_ns: wall / 2 } } });
  const program = {
    id: 'bench300k', source: { code_lines: 300000, physical_lines: 320000 },
    arms: [{ id: 'default', samples: [sample(1e9, 'a')], summary: summary(1e9, 1) }, { id: 'single', samples: [sample(3e9, 'b')], summary: summary(3e9, 1) }],
  };
  const manifest = JSON.parse(fs.readFileSync(path.join(here, '../../gauntlet/measurement-manifest.json'), 'utf8'));
  const cell = manifest.axes.compile_throughput;
  const gates = cellGates(cell, [program]);
  assert.equal(gates.results.bench300k.determinism, 'fail');
  assert.equal(gates.results.bench300k.threshold, 'pass');
  assert.ok(gates.failures.some(line => /phase over budget: read, lex, parse/.test(line)));
  const budgets = phaseBudgetCheck({ ...summary(1e9, 1), startup_ns: 0 }, 30000);
  assert.equal(budgets.scale, 0.1);
  assert.equal(budgets.rows.find(row => row.row === 2).budget_ns, 8e6);
  const receipt = { machine: { cpu_model: 'x', threads: 2, memory_bytes: 1 }, compiler: { kind: 'jet' }, programs: [{ id: 'bench30k', source: { sha256: 's' }, arms: [{ id: 'default', summary: { ...summary(2e9, 100), phases: { load: { wall_ns: 1.2e9 }, tiny: { wall_ns: 9e7 } } } }] }] };
  const baseline = { schema: 'jet.compile-throughput.quick-baseline/v1', machine: receipt.machine, compiler_kind: 'jet', programs: { bench30k: { source_sha256: 's', arms: { default: { frontend_wall_ns: 1.8e9, peak_rss_bytes: 100, phases: { load: 1e9, tiny: 1e6 } } } } } };
  const compared = quickCompare(baseline, receipt, { latency_regression_pct: 15, memory_regression_pct: 15 });
  assert.deepEqual(compared.failures, ['bench30k/default phase load: 1200000000 vs baseline 1000000000 (budget +15%)']);
  assert.equal(compared.comparisons.find(row => row.metric === 'phase tiny').status, 'below-floor');
  assert.match(quickCompare(null, receipt, {}).failures[0], /missing/);
  assert.match(quickCompare({ ...baseline, programs: { bench30k: { ...baseline.programs.bench30k, source_sha256: 'other' } } }, receipt, { latency_regression_pct: 15 }).failures[0], /baseline source/);
});

// A stand-in `jet` that writes a two-phase trace and an executable printing
// GOLDEN (or a wrong line), so the harness's real process, trace, receipt
// and validation paths run without a compiler.
function fakeJet(name, output) {
  const file = path.join(root, name);
  fs.writeFileSync(`${file}.out`, output);
  fs.writeFileSync(file, `#!/usr/bin/env bash
set -eu
now=$(date +%s%3N)
printf '[\\n{"name":"trace.start","ph":"i","ts":0,"pid":1,"tid":0,"args":{"unix_ms":%s}},\\n{"name":"load","cat":"load","ph":"B","ts":1000,"pid":1,"tid":1},\\n{"name":"load","cat":"load","ph":"E","ts":2000,"pid":1,"tid":1},\\n' "$now" > "$JET_TRACE_FILE"
mkdir -p .jet/build
printf '#!/bin/sh\\nexec cat %s\\n' '${file}.out' > .jet/build/main
chmod +x .jet/build/main
`);
  fs.chmodSync(file, 0o755);
  return file;
}

test('the harness writes a receipt, and a wrong output or missing baseline fails the run', () => {
  const harness = path.join(here, 'harness.mjs');
  const golden = PINS.programs.bench30k.expected_output;
  const args = (compiler, extra) => [harness, '--compiler', `jet=${compiler}`, '--compiler-profile', 'test stand-in', '--samples', '2', '--warmups', '0',
    '--memory-max', '1G', '--work-dir', path.join(root, 'work'), ...extra];
  const baselineFile = path.join(root, 'baseline.json');
  const write = spawnSync(process.execPath, args(fakeJet('good-jet', golden), ['--write-baseline', baselineFile, '--receipt', path.join(root, 'good.json')]), { encoding: 'utf8' });
  assert.equal(write.status, 0, `${write.stdout}\n${write.stderr}`);
  const receipt = JSON.parse(fs.readFileSync(path.join(root, 'good.json'), 'utf8'));
  assert.equal(receipt.passed, true);
  const arm = receipt.programs[0].arms[0];
  assert.equal(arm.samples.length, 2);
  for (const key of ['wall_ns', 'cpu_user_ns', 'cpu_sys_ns', 'peak_rss_bytes', 'threads_used', 'artifact_sha256', 'diagnostic_sha256']) assert.notEqual(arm.samples[0][key], undefined, key);
  assert.equal(arm.samples[0].phases.load.count, 1);
  assert.equal(receipt.programs[0].source.code_lines, PINS.programs.bench30k.shape.code_lines);
  assert.equal(receipt.compiler.build_profile.description, 'test stand-in');
  assert.ok(fs.existsSync(baselineFile));

  const again = spawnSync(process.execPath, args(fakeJet('good-jet-2', golden), ['--baseline', baselineFile]), { encoding: 'utf8' });
  assert.equal(again.status, 0, `${again.stdout}\n${again.stderr}`);

  const wrong = spawnSync(process.execPath, args(fakeJet('wrong-jet', 'mixed checksum 1 sections 31\n'), ['--baseline', baselineFile]), { encoding: 'utf8' });
  assert.equal(wrong.status, 1);
  assert.match(wrong.stderr, /differs from the golden/);
  assert.match(wrong.stderr, /samples missing or failed/);

  const missing = spawnSync(process.execPath, args(fakeJet('good-jet-3', golden), ['--baseline', path.join(root, 'absent.json')]), { encoding: 'utf8' });
  assert.equal(missing.status, 1);
  assert.match(missing.stderr, /quick-lane baseline is missing/);
});
