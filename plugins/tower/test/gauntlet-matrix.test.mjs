import { test } from 'node:test';
import assert from 'node:assert/strict';
import { buildGauntletTooltip, projectGauntletMatrix } from '../app/ui/gauntlet.js';

const tier = (ratio, verdict = null) => ({
  status: 'measured', jet: ratio, peer: 1, ratio, verdict,
  samples: { jet: [ratio], peer: [1] },
  median: { jet: ratio, peer: 1 }, p99: { jet: ratio, peer: 1 }, rss: { jet: 10, peer: 12 },
});

function fixture() {
  return {
    summary: { win: 1, parity: 0, loss: 1, unmeasured: 1 },
    cells: [
      {
        id: 'text.kernel', domain: 'text', entry: 'wordfreq', mode: 'batch',
        primary_metric: 'runtime_wall_seconds', verdict: 'loss', measured_at: '2026-09-02', run_id: 'run-2',
        failures: [{ card: 123, metric: 'runtime_wall_seconds' }], loss_owners: [{ card_number: 456 }],
        peers: [
          { peer: 'rust', required_tiers: ['aot'], metric_comparisons: { runtime_wall_seconds: { tiers: { aot: tier(1.02, 'parity') } } } },
          { peer: 'python', required_tiers: ['aot'], metric_comparisons: { runtime_wall_seconds: { tiers: { aot: tier(0.71, 'win') } } } },
        ],
      },
      { id: 'empty.cell', domain: 'other', entry: null, mode: 'batch', primary_metric: 'runtime_wall_seconds', verdict: 'unmeasured', peers: [] },
    ],
    axes: {
      live_reload: {
        metric: 'reload_latency_ms',
        comparisons: { vite: { cold: tier(1.1, 'loss'), warm: tier(0.9, 'win') } },
      },
    },
  };
}

test('projects cells, rails, domain groups, and axes into a matrix', () => {
  const matrix = projectGauntletMatrix(fixture());
  assert.deepEqual(matrix.columns, ['rust', 'python', 'vite']);
  assert.deepEqual(matrix.groups.map((group) => group.label), ['other', 'text', 'Axes']);
  const text = matrix.rows.find((row) => row.id === 'text.kernel');
  assert.equal(text.peers.rust.verdict, 'parity');
  assert.equal(text.peers.rust.ratio, 1.02);
  assert.equal(text.peers.python.verdict, 'win');
  assert.equal(text.peers.vite, undefined);
  const axis = matrix.axisRows[0];
  assert.equal(axis.mode, 'live_reload');
  assert.equal(axis.peers.vite.verdict, 'loss');
  assert.equal(axis.peers.vite.tier, 'cold');
  assert.equal(matrix.rows.find((row) => row.id === 'empty.cell').peers.rust, undefined);
});

test('builds complete, escaped tooltip details with card links', () => {
  const row = projectGauntletMatrix(fixture()).rows.find((item) => item.id === 'text.kernel');
  const html = buildGauntletTooltip(row.peers.python.detail);
  assert.match(html, /runtime wall seconds \(seconds\)/);
  assert.match(html, /Jet \/ python &lt; 1\.00 wins/);
  assert.match(html, /data-card="123"/);
  assert.match(html, /data-card="456"/);
  assert.match(html, /Samples/);
  assert.doesNotMatch(html, /undefined/);
  assert.doesNotMatch(html, /null/);
});
