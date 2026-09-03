import { test } from 'node:test';
import assert from 'node:assert/strict';
import { buildGauntletTooltip, formatPair, formatRatio, formatStamp, projectGauntletMatrix } from '../app/ui/gauntlet.js';

const tier = (ratio, verdict = null) => ({
  status: 'measured', jet: ratio, peer: 1, ratio, verdict,
  measured_iso: '2026-09-02T17:53:13.150Z', run_id: 'run-2',
  stats: { jet: { samples: 3, best: ratio * 0.9, median: ratio, mean: ratio * 1.05, rss: 318000 }, peer: { samples: 3, best: 0.9, median: 1, mean: 1.1, rss: 14200 } },
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
          { peer: 'rust', required_tiers: ['aot'], metric_comparisons: { runtime_wall_seconds: { tiers: { aot: tier(1.02, 'loss') } } } },
          { peer: 'python', required_tiers: ['aot'], metric_comparisons: { runtime_wall_seconds: { tiers: { aot: tier(0.71, 'loss') } } } },
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

test('projects cells and axes into one fixed matrix in domain order', () => {
  const matrix = projectGauntletMatrix(fixture());
  assert.deepEqual(matrix.columns, ['rust', 'python']);
  assert.deepEqual(matrix.axisColumns, ['vite']);
  assert.deepEqual(matrix.rows.map((row) => row.id), ['text.kernel', 'empty.cell', 'axis:live_reload']);
  const text = matrix.rows.find((row) => row.id === 'text.kernel');
  assert.equal(text.peers.rust.verdict, 'parity');
  assert.equal(text.peers.rust.ratio, 1.02);
  assert.equal(text.peers.python.verdict, 'win');
  assert.equal(text.peers.python.detail.verdict, 'win');
  assert.equal(text.peers.vite, undefined);
  const axis = matrix.axisRows[0];
  assert.equal(axis.mode, 'live_reload');
  assert.equal(axis.peers.vite.verdict, 'loss');
  assert.equal(axis.peers.vite.tier, 'cold');
  assert.equal(matrix.rows.find((row) => row.id === 'empty.cell').peers.rust, undefined);
});

test('formats both sides in one plain unit at three significant figures', () => {
  assert.deepEqual(formatPair('runtime_wall_seconds', 24.926576, 0.0146416), { jet: '24,900 ms', peer: '14.6 ms', unit: 'ms' });
  assert.deepEqual(formatPair('runtime_wall_seconds', 0.0185680, 0.0487236), { jet: '18.6 ms', peer: '48.7 ms', unit: 'ms' });
  assert.deepEqual(formatPair('runtime_peak_rss_kb', 584732, 14228), { jet: '585 MB', peer: '14.2 MB', unit: 'MB' });
  assert.deepEqual(formatPair('runtime_wall_seconds', null, 0.0000042), { jet: '—', peer: '4.2 µs', unit: 'µs' });
  assert.equal(formatRatio(5103.77797), '5,100×');
  assert.equal(formatRatio(0.38108), '0.381×');
  assert.equal(formatRatio(null), '—');
  assert.match(formatStamp('2026-09-02T17:53:13.150Z'), /^Sep 2, 2026 · \d{2}:\d{2}$/);
  assert.equal(formatStamp(null, '2026-09-01'), 'Sep 1, 2026');
  assert.equal(formatStamp(null, null), 'not measured');
});

test('builds the detail card: every tier, best/median/mean, memory, and stamp', () => {
  const row = projectGauntletMatrix(fixture()).rows.find((item) => item.id === 'text.kernel');
  const html = buildGauntletTooltip(row.peers.python.detail);
  assert.match(html, /TEXT\.KERNEL|text\.kernel/);
  assert.match(html, /wordfreq · Jet vs python · wall time/);
  assert.match(html, /gtip__state--win/);
  assert.match(html, /<th>aot<\/th><td>710 ms<\/td><td>1,000 ms<\/td><td class="gtip__ratio">0\.71×<\/td>/);
  assert.match(html, /<th>best<\/th>[\s\S]*<th>median<\/th>[\s\S]*<th>mean<\/th>[\s\S]*<th>peak memory<\/th><td>318 MB<\/td><td>14\.2 MB<\/td>[\s\S]*<th>samples<\/th><td>3<\/td><td>3<\/td>/);
  assert.match(html, /Sep 2, 2026 · \d{2}:\d{2}/);
  assert.match(html, /run run-2/);
  assert.doesNotMatch(html, /\d[eE][+-]\d/, 'no exponent notation');
  for (const banned of ['Mode', 'Rule', 'Failures', 'Loss owners', 'undefined', 'null']) {
    assert.doesNotMatch(html, new RegExp(banned), banned);
  }
});

test('keeps a published peer verdict when a legacy row has no metric tiers', () => {
  const matrix = projectGauntletMatrix({
    cells: [{
      id: 'legacy.cell',
      domain: 'legacy',
      primary_metric: 'runtime_wall_seconds',
      peers: [{ peer: 'rust', verdict: 'parity', metric_comparisons: {} }],
    }],
  });
  assert.equal(matrix.rows[0].peers.rust.verdict, 'parity');
});

test('shows the tier that determines an aggregate loss', () => {
  const matrix = projectGauntletMatrix({
    cells: [{
      id: 'tiered.cell',
      domain: 'tiered',
      primary_metric: 'runtime_wall_seconds',
      peers: [{
        peer: 'python',
        required_tiers: ['aot', 'run'],
        metric_comparisons: {
          runtime_wall_seconds: { tiers: { aot: tier(0.7, 'win'), run: tier(1.2, 'loss') } },
        },
      }],
    }],
  });
  const peer = matrix.rows[0].peers.python;
  assert.deepEqual([peer.verdict, peer.tier, peer.ratio], ['loss', 'run', 1.2]);
});
