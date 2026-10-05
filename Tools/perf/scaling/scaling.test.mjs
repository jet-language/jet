import assert from 'node:assert/strict';
import test from 'node:test';
import { SHAPES, corpus } from './corpora.mjs';
import { growth, judge, metricPhase, parseCounters, parseCompiler } from './harness.mjs';

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
