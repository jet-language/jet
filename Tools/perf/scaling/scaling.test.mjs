import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { AXES, generate, sources } from './generate.mjs';
import { compare, DEFAULT_BASES, DEFAULT_JET, GNU_TIME, growth, parseCounters, render, validateOverrides } from './harness.mjs';

const scratch = process.env.JET_SCALING_SCRATCH ?? path.join(os.homedir(), '.cache/jet-dev/scratch/sol/SolScaleGate');
fs.mkdirSync(scratch, { recursive: true });
const root = fs.mkdtempSync(path.join(scratch, 'test-'));
const here = path.dirname(fileURLToPath(import.meta.url));

// Run the whole test through laneS; every compiler child inherits its memory cap.
test('every axis produces deterministic, valid Jet at the smallest size', () => {
  const jet = process.env.JET ?? DEFAULT_JET;
  for (const axis of AXES) {
    assert.deepEqual(sources(axis, 1), sources(axis, 1));
    assert.notDeepEqual(sources(axis, 1), sources(axis, 2));
    for (const n of [1, 3]) {
      const directory = path.join(root, axis, String(n));
      const source = generate(axis, n, directory);
      const result = spawnSync(jet, ['check', source], { cwd: directory, encoding: 'utf8' });
      assert.equal(result.status, 0, `${axis}, N=${n}: ${result.error?.message ?? ''}\n${result.stdout}\n${result.stderr}`);
      assert.throws(() => generate(axis, n, directory), /already exists/);
    }
  }
  assert.throws(() => sources('unknown', 1), /Unknown axis/);
  assert.throws(() => sources('functions', 0), /positive/);
});

test('independent source dimensions', () => {
  const functionCount = text => (text.match(/\bfn /g) ?? []).length;
  for (const axis of AXES.filter(axis => axis !== 'functions')) {
    assert.equal(functionCount(sources(axis, 1)['main.jet']), functionCount(sources(axis, 4)['main.jet']), axis);
  }
  assert.equal(Object.keys(sources('modules', 4)).length, 5);
  assert.equal(functionCount(sources('functions', 4)['main.jet']), 5);
  assert.equal((sources('declarations', 6)['main.jet'].match(/\b(struct|enum|alias) /g) ?? []).length, 6);
  assert.equal((sources('interpolation', 4)['main.jet'].match(/\{x\}/g) ?? []).length, 4);
  assert.equal((sources('generics', 4)['main.jet'].match(/probe<\[Int#/g) ?? []).length, 4);
  assert.equal((sources('fan-in', 4)['main.jet'].match(/total \+= shared/g) ?? []).length, 4);
});

test('counter parsing, exemptions, missing counters, and zero baselines fail closed', () => {
  assert.deepEqual(parseCounters('noise\nJET_PHASE_COUNTERS {"phase":"parse","visits":2,"alloc":{"bytes":4}}\nJET_PHASE_COUNTERS: {"phase":"parse","visits":3}'),
    { 'parse.alloc.bytes': 4, 'parse.visits': 5 });
  assert.throws(() => parseCounters('JET_PHASE_COUNTERS not-json'), /Malformed/);
  assert.throws(() => parseCounters('JET_PHASE_COUNTERS {"visits":-1}'), /Invalid/);
  assert.equal(growth(0, 0), 1);
  assert.equal(growth(0, 1), 'Infinity');
  const points = [1, 2, 4].map(n => ({ n, metrics: { visits: n * n, linear: n } }));
  const zero = { n: 0, metrics: { visits: 0, linear: 0 } };
  assert.equal(compare(points, 2.2, zero).failures.length, 2);
  assert.equal(compare(points, 4, zero).failures.length, 0);
  assert.equal(compare([{ n: 1, metrics: {} }, { n: 2, metrics: { visits: 1 } }], 2.2, { metrics: {} }).failures.length, 1);
  const startup = { n: 1, metrics: { work: 100 } };
  const marginal = [{ n: 256, metrics: { work: 110 } }, { n: 512, metrics: { work: 140 } }];
  assert.equal(compare(marginal, 2.2, startup).ratios[0].metrics.work, 4);
  assert.equal(compare(marginal, 2.2, startup).failures.length, 1);
  assert.equal(compare([{ n: 256, metrics: { work: 99 } }, marginal[1]], 2.2, startup).ratios[0].metrics.work, null);
  const boundary = compare([{ n: 256, metrics: { user_seconds: 1.08 } }, { n: 512, metrics: { user_seconds: 1.14 } }], 2.2,
    { n: 1, metrics: { user_seconds: 1.03 } });
  assert.equal(boundary.ratios[0].metrics.user_seconds, 2.2);
  assert.equal(boundary.failures.length, 0);
  assert.equal(DEFAULT_BASES.depth * 4, 64);
  assert.equal(DEFAULT_BASES['match-arms'] * 4, 192);
  assert.ok(AXES.every(axis => DEFAULT_BASES[axis] > 1));
  assert.throws(() => validateOverrides({ depth: { bound: 4 } }), /reason/);
  assert.deepEqual(validateOverrides({ depth: { bound: 4, reason: 'Documented algorithm' } }).depth.bound, 4);
  assert.equal(render('tool {source}', { source: "a'b" }), "tool 'a'\\''b'");
  assert.throws(() => render('{unknown}', {}), /Unknown/);
});

test('CLI writes a real timed receipt and exits 1 for quadratic counters', () => {
  const fixture = path.join(root, 'counter-compiler.mjs');
  fs.writeFileSync(fixture, 'const n = Number(process.argv[2]); console.log(`JET_PHASE_COUNTERS ${JSON.stringify({phase:"parse",visits:n*n})}`);\n');
  const receipt = path.join(root, 'quadratic.json');
  const result = spawnSync(process.execPath, [path.join(here, 'harness.mjs'), '--axis', 'functions', '--n', '2', '--repeats', '1',
    '--time', GNU_TIME, '--work-dir', root, '--receipt', receipt,
    '--command', `fixture=${JSON.stringify(process.execPath)} ${JSON.stringify(fixture)} {n}`], { encoding: 'utf8' });
  assert.equal(result.status, 1, `${result.stdout}\n${result.stderr}`);
  const data = JSON.parse(fs.readFileSync(receipt, 'utf8'));
  assert.equal(data.passed, false);
  assert.equal(data.baselineN, 1);
  assert.equal(data.results[0].baseline.metrics['counter.parse.visits'], 1);
  assert.equal(data.results[0].ratios[0].metrics['counter.parse.visits'], 5);
  assert.equal(data.results[0].ratios[1].metrics['counter.parse.visits'], 4.2);
  assert.match(result.stdout, /axis\s+compiler/);
  assert.match(result.stderr, /counter.parse.visits/);
});
