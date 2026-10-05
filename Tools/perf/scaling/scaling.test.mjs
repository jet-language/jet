import assert from 'node:assert/strict';
import test from 'node:test';
import { SHAPES, corpus } from './corpora.mjs';
import { floors, growth, judge, metricPhase, options, parseCounters, parseCompiler, resumeReceipt, run } from './harness.mjs';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { report } from './report.mjs';

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

test('resume preserves completed rungs, including timeouts, without retrying them', async () => {
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
    assert.deepEqual(resumed.series[0].points, [timedOut]);
    assert.equal(resumed.failures.length, 1);
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
  assert.ok(text.includes('modules | files | a | sema.bodies'));
  assert.ok(text.includes('modules | files | b | sema.bodies'));
});
