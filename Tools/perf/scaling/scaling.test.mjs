import assert from 'node:assert/strict';
import test from 'node:test';
import { SHAPES, corpus } from './corpora.mjs';
import { concurrent, floors, growth, judge, metricPhase, options, parseCounters, parseCompiler, resumeReceipt, run } from './harness.mjs';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { report } from './report.mjs';
import { queuePlan } from './enqueue.mjs';
import { pack } from './package.mjs';

test('every shape is deterministic and grows with N', () => {
  for (const [shape, entry] of Object.entries(SHAPES)) {
    const small = entry.kind === 'domain' ? 800 : Math.max(8, entry.base / 8);
    assert.equal(corpus(shape, small).source_sha256, corpus(shape, small).source_sha256, shape);
    assert.ok(corpus(shape, small * 2).code_lines > corpus(shape, small).code_lines, shape);
    assert.ok(corpus(shape, small).files['main.jet'].includes('fn run()'), shape);
  }
  assert.equal(corpus('diagnostics', 5).expect.diagnostics, 5);
  assert.throws(() => corpus('unknown', 1), /Unknown shape/);
});

test('large packed items and single-file domains do not expand lines into call arguments', () => {
  const files = {};
  const lines = Array.from({ length: 200000 }, () => 'row');
  pack(files, 'large', [lines]);
  assert.equal(files['large_0.jet'], `${lines.join('\n')}\n`);
  const large = corpus('generic-lib', 200000, 1, 'single');
  assert.ok(large.code_lines >= 200000);
  assert.ok(large.files['main.jet'].includes('fn run()'));
});

test('counter parsing fails closed', () => {
  assert.deepEqual(parseCounters('noise\nJET_PHASE_COUNTERS {"phase":"parse","visits":2}\nJET_PHASE_COUNTERS {"phase":"parse","visits":3}'), { 'parse.visits': 5 });
  assert.throws(() => parseCounters('JET_PHASE_COUNTERS not-json'), /Malformed/);
  assert.throws(() => parseCounters('JET_PHASE_COUNTERS {"visits":-1}'), /Invalid/);
});

test('growth against N log N names the phase and its dominating counter', () => {
  const sizes = [1000, 2000, 4000, 8000];
  assert.ok(growth(sizes, sizes.map(x => x * 3), 1).ratio <= 1);
  assert.ok(growth(sizes, sizes.map(x => x * x), 1).ratio > 3);
  assert.equal(metricPhase('counter.sema.check.scope_lookups'), 'sema.check');
  const point = (x, q) => ({ ok: true, x, metrics: { 'phase.sema.check': q * x * x / 1e6, 'counter.sema.check.scope_lookups': x * x, 'counter.sema.check.tokens': x } });
  const judged = judge({ baseline: { ok: true, metrics: {} }, points: sizes.map(x => point(x, 1)) });
  const failure = judged.failures.find(f => f.metric === 'phase.sema.check');
  assert.equal(failure.counter, 'counter.sema.check.scope_lookups');
  assert.ok(!judged.failures.some(f => f.metric === 'counter.sema.check.tokens'));
  assert.throws(() => parseCompiler('bad'), /LABEL=/);
});

test('timing resume preserves a timeout and attempts every remaining rung without retrying it', async () => {
  const scratch = path.join(os.homedir(), '.cache/jet-dev/scratch');
  fs.mkdirSync(scratch, { recursive: true });
  const dir = fs.mkdtempSync(path.join(scratch, 'scaling-resume-'));
  try {
    const config = options(['--compiler', 'fixture=command:exit 99', '--shape', 'modules', '--layout', 'single', '--ladder', 'modules=2,4', '--work-dir', dir]);
    const receipt = resumeReceipt(config, {});
    const timedOut = { n: 2, x: 2, ok: false, problems: ['timed out in sema.bodies'], open_phases: ['sema.bodies'], timeout_phase: 'sema.bodies', metrics: {} };
    receipt.series.push({ shape: 'modules', compiler: 'fixture', layout: 'single', baseline: { ok: true, metrics: {} }, points: [timedOut], truncated: null });
    assert.equal(resumeReceipt(config, {}, receipt), receipt);
    assert.throws(() => resumeReceipt({ ...config, seed: 2 }, {}, receipt), /invocation differs/);
    assert.throws(() => resumeReceipt({ ...config, buildRoot: '/different-build-root' }, {}, receipt), /invocation differs/);
    // The checkpoint has a failed rung but no series verdict yet: do not retry it.
    receipt.invocation = resumeReceipt(config, floors()).invocation;
    fs.writeFileSync(config.receipt, JSON.stringify(receipt));
    const resumed = await run(config);
    assert.deepEqual(resumed.series[0].points[0], timedOut);
    assert.equal(resumed.series[0].points.length, 2);
    assert.equal(resumed.series[0].points[1].n, 4);
    assert.equal(resumed.series[0].points[1].ok, false);
    assert.equal(resumed.series[0].truncated, null);
    assert.equal(resumed.failures.length, 2);
    assert.equal(resumed.series[0].failures[0].kind, 'timeout');
    assert.equal(resumed.series[0].failures[0].phase, 'sema.bodies');
    assert.equal(resumed.series[0].failures[1].kind, 'compile');
    assert.equal(resumed.passed, false);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test('resumed counter failures retain the last open phase without claiming a timeout', async () => {
  const scratch = path.join(os.homedir(), '.cache/jet-dev/scratch');
  fs.mkdirSync(scratch, { recursive: true });
  const dir = fs.mkdtempSync(path.join(scratch, 'scaling-crash-phase-'));
  try {
    const config = options(['--compiler', 'fixture=command:exit 99', '--shape', 'modules', '--layout', 'single',
      '--mode', 'counters', '--jobs', '2', '--ladder', 'modules=2,4', '--work-dir', dir]);
    const failed = n => ({ n, x: n, ok: false, problems: ['compiler exited 139'], open_phases: ['compile', 'sema.check'], timeout_phase: null, metrics: {} });
    const receipt = resumeReceipt(config, floors());
    receipt.series.push({ shape: 'modules', compiler: 'fixture', layout: 'single', mode: 'counters',
      baseline: failed(1), points: [failed(2), failed(4)], truncated: null });
    fs.writeFileSync(config.receipt, JSON.stringify(receipt));
    const resumed = await run(config);
    assert.equal(resumed.series[0].failures.length, 3);
    assert.ok(resumed.series[0].failures.every(failure => failure.kind === 'compile' && failure.phase === 'sema.check'));
    assert.equal(resumed.passed, false);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test('reports merge receipts and retain timeout phase attribution', () => {
  const receipt = compiler => ({ startedAt: 'start', finishedAt: 'end', compilers: [{ label: compiler, path: '/compiler' }], failures: ['timeout'],
    series: [{ shape: 'modules', layout: 'files', compiler, points: [], done: true, truncated: 'timed out',
      failures: [{ kind: 'timeout', phase: 'sema.bodies', text: 'timed out after 120 s in sema.bodies' }] }] });
  const text = report([{ file: 'a.json', receipt: receipt('a') }, { file: 'b.json', receipt: receipt('b') }]);
  assert.ok(text.includes('modules | files | a | timing | sema.bodies'));
  assert.ok(text.includes('modules | files | b | timing | sema.bodies'));
});

test('counter verdicts exclude contended wall/RSS metrics', () => {
  const points = [100, 200, 400, 800].map(x => ({ x, ok: true,
    metrics: { 'counter.sema.bodies.items_scanned': x, wall_seconds: x * x, peak_rss_kb: x * x * 1000 } }));
  const timed = judge({ points });
  assert.ok(timed.failures.some(failure => failure.metric === 'wall_seconds'));
  const counted = judge({ points, mode: 'counters' });
  assert.equal(counted.failures.length, 0);
  assert.deepEqual(Object.keys(counted.metrics), ['counter.sema.bodies.items_scanned']);
});

test('counter rung concurrency is bounded and visits every input', async () => {
  let active = 0;
  let peak = 0;
  const visited = [];
  await concurrent([1, 2, 3, 4, 5], 2, async value => {
    active += 1;
    peak = Math.max(peak, active);
    await new Promise(resolve => setImmediate(resolve));
    visited.push(value);
    active -= 1;
  });
  assert.equal(peak, 2);
  assert.deepEqual(visited.sort(), [1, 2, 3, 4, 5]);
});

test('queue plan covers all ladders in three shareable groups and one timing sample', () => {
  const plan = queuePlan(options(['--compiler', 'rust=jet:/compiler', '--work-dir', '/evidence']));
  const counted = plan.filter(job => job.config.mode === 'counters');
  assert.equal(counted.length, 3);
  assert.deepEqual(counted.flatMap(job => job.config.shapes).sort(), Object.keys(SHAPES).sort());
  assert.ok(counted.every(job => !job.name.startsWith('Perf') && job.shared && job.config.jobs === 2 && job.config.timeoutS === 25));
  assert.equal(new Set(plan.map(job => job.config.workDir)).size, 4);
  const timed = plan.filter(job => job.config.mode === 'timing');
  assert.equal(timed.length, 1);
  assert.equal(timed[0].name, 'PerfScalingGate-timing');
  assert.equal(timed[0].shared, false);
  assert.ok(timed[0].config.shapes.includes('fan-in'));
  assert.deepEqual(timed[0].config.domainLadder, [125000, 1000000]);
  assert.deepEqual(timed[0].config.shapes.filter(shape => SHAPES[shape].kind === 'domain').sort(),
    Object.keys(SHAPES).filter(shape => SHAPES[shape].kind === 'domain').sort());
});

test('counter reports never present contended wall/RSS as timing evidence', () => {
  const receipt = { mode: 'counters', startedAt: 'start', finishedAt: 'end', compilers: [{ label: 'rust', path: '/compiler' }], failures: [],
    series: [{ shape: 'modules', layout: 'single', compiler: 'rust', mode: 'counters', done: true, failures: [],
      points: [{ x: 1000, ok: true, metrics: { wall_seconds: 999996, peak_rss_kb: 999996 } }] }] };
  const text = report([{ file: 'counter.json', receipt }]);
  assert.ok(text.includes('modules | single | rust | counters | 1000 | - | -'));
  assert.ok(text.includes('not in the timing sample'));
  assert.ok(!text.includes('999996'));
});
