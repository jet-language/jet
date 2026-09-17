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
          { peer: 'python', required_tiers: ['aot'], metric_comparisons: { runtime_wall_seconds: { tiers: { aot: tier(9), run: tier(0.71), dev: tier(0.8) } } } },
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

test('groups compiled and dynamic peers using their assigned execution samples', () => {
  const matrix = projectGauntletMatrix(fixture());
  assert.deepEqual(matrix.columns, ['rust', 'python']);
  assert.deepEqual(matrix.groups, [{ label: 'AOT', columns: ['rust'] }, { label: 'Run / dev', columns: ['python'] }]);
  assert.deepEqual(matrix.axisColumns, ['bun', 'nodemon', 'vite']);
  assert.deepEqual(matrix.rows.map((row) => row.id), ['text.kernel', 'empty.cell', 'axis:live_reload']);
  const text = matrix.rows.find((row) => row.id === 'text.kernel');
  assert.equal(text.peers.rust.verdict, 'parity');
  assert.equal(text.peers.rust.ratio, 1.02);
  assert.equal(text.peers.python.verdict, 'win');
  assert.equal(text.peers.python.detail.verdict, 'win');
  assert.deepEqual(text.peers.python.samples.map(({ tier, ratio }) => [tier, ratio]), [['run', 0.71], ['dev', 0.8]]);
  assert.equal(text.verdict, 'parity');
  assert.equal(text.peers.vite, undefined);
  const axis = matrix.axisRows[0];
  assert.equal(axis.mode, 'live_reload');
  assert.equal(axis.peers.vite.verdict, 'loss');
  assert.equal(axis.peers.vite.tier, 'cold');
  assert.equal(axis.peers.nodemon.verdict, 'unmeasured');
  assert.equal(axis.verdict, 'unmeasured');
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


test('does not reuse a historical aggregate when the assigned tier has no measurement', () => {
  const matrix = projectGauntletMatrix({
    cells: [{
      id: 'unmeasured.cell',
      primary_metric: 'runtime_wall_seconds',
      peers: [{ peer: 'rust', verdict: 'parity', metric_comparisons: {} }],
    }],
  });
  assert.equal(matrix.rows[0].peers.rust.verdict, 'unmeasured');
  assert.equal(matrix.rows[0].peers.rust.ratio, null);
});

test('a compiled-peer run loss cannot replace its AOT score', () => {
  const matrix = projectGauntletMatrix({
    cells: [{
      id: 'tiered.cell',
      domain: 'tiered',
      primary_metric: 'runtime_wall_seconds',
      peers: [{
        peer: 'rust',
        required_tiers: ['aot', 'run'],
        metric_comparisons: {
          runtime_wall_seconds: { tiers: { aot: tier(0.7, 'win'), run: tier(1.2, 'loss') } },
        },
      }],
    }],
  });
  const peer = matrix.rows[0].peers.rust;
  assert.deepEqual([peer.verdict, peer.tier, peer.ratio], ['win', 'aot', 0.7]);
  assert.equal(peer.detail.peer.metric_comparisons.runtime_wall_seconds.tiers.run.ratio, 1.2);
});

test('a missing dev measurement stays visible beside a measured run', () => {
  const matrix = projectGauntletMatrix({
    cells: [{
      id: 'dynamic.cell', primary_metric: 'runtime_wall_seconds',
      peers: [{ peer: 'python', required_tiers: ['aot', 'run'],
        metric_comparisons: { runtime_wall_seconds: { tiers: { aot: tier(0.1), run: tier(0.5) } } } }],
    }],
  });
  const peer = matrix.cellRows[0].peers.python;
  assert.equal(peer.verdict, 'unmeasured');
  assert.deepEqual(peer.samples.map(({ tier, ratio, verdict }) => [tier, ratio, verdict]),
    [['run', 0.5, 'win'], ['dev', null, 'unmeasured']]);
});

test('JavaScript occupies one column without picking the faster duplicate record', () => {
  const peer = (name, ratio) => ({ peer: name, metric_comparisons: {
    runtime_wall_seconds: { tiers: { aot: tier(ratio), run: tier(ratio), dev: tier(ratio) } },
  } });
  const source = { cells: [{
    id: 'js.cell', primary_metric: 'runtime_wall_seconds',
    peers: [peer('node', 0.2), peer('js', 1.4)],
  }, {
    id: 'node-only.cell', primary_metric: 'runtime_wall_seconds', peers: [peer('node', 0.3)],
  }] };
  const snapshot = JSON.stringify(source);
  const matrix = projectGauntletMatrix(source);
  assert.deepEqual(matrix.columns, ['js']);
  const js = matrix.cellRows.find((row) => row.id === 'js.cell').peers.js;
  assert.equal(js.ratio, 1.4);
  assert.equal(js.verdict, 'loss');
  assert.equal(matrix.cellRows.find((row) => row.id === 'node-only.cell').peers.js.ratio, 0.3);
  const tooltip = buildGauntletTooltip(js.detail);
  assert.ok(tooltip.includes('node · run · reference'));
  assert.ok(tooltip.includes('0.2×'));
  assert.equal(JSON.stringify(source), snapshot);
});

test('web development retains its tool peers without gating on compiled workflow speed', () => {
  const source = { axes: { live_reload: {
    metric: 'reload_latency_ms', comparisons: Object.fromEntries(
      ['bun', 'nodemon', 'vite', 'entr+cc'].map((name) => [name, {
        cold: tier(name === 'entr+cc' ? 10 : 0.5), warm: tier(0.5),
      }])),
  } } };
  const matrix = projectGauntletMatrix(source);
  assert.equal(matrix.axisRows[0].verdict, 'win');
  assert.equal(matrix.axisRows[0].peers['entr+cc'].verdict, 'loss');
  assert.equal(matrix.axisRows[0].peers['entr+cc'].detail.reference, true);
  source.axes.live_reload.comparisons.nodemon.warm = tier(2);
  assert.equal(projectGauntletMatrix(source).axisRows[0].verdict, 'loss');
});

test('invalid samples cannot turn a favorable ratio into a win', () => {
  const matrix = projectGauntletMatrix({ cells: [{
    id: 'wrong.cell', primary_metric: 'runtime_wall_seconds', peers: [{
      peer: 'rust', metric_comparisons: {
        runtime_wall_seconds: { tiers: { aot: { ...tier(0.1), status: 'wrong' } } },
      },
    }],
  }] });
  const peer = matrix.cellRows[0].peers.rust;
  assert.equal(peer.verdict, 'unmeasured');
  assert.equal(peer.samples[0].ratio, null);
});

test('expert measurements do not add columns or alter the displayed ordinary score', () => {
  const source = { cells: [{
    id: 'expert.cell', primary_metric: 'runtime_wall_seconds',
    peers: ['rust', 'rust-expert', 'c-expert', 'jet-expert'].map((name) => ({
      peer: name, metric_comparisons: {
        runtime_wall_seconds: { tiers: { aot: tier(name === 'rust' ? 0.5 : 20) } },
      },
    })),
  }] };
  const snapshot = JSON.stringify(source);
  const matrix = projectGauntletMatrix(source);
  assert.deepEqual(matrix.columns, ['rust']);
  assert.equal(matrix.cellRows[0].verdict, 'win');
  assert.equal(matrix.summary.win, 1);
  assert.equal(JSON.stringify(source), snapshot);
});
