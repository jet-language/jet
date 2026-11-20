// #6834: the standings projection the Gauntlet page and `tower gauntlet
// standings` share, for a peers-only run (Jet pending), a run with Jet
// merged and the planned grid, the gauntlet table, and the CLI table.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { empty, TowerError } from '../App/store.mjs';
import { writeJSON } from '../App/paths.mjs';
import { readGauntletStandings, tableStandings } from '../App/gauntlet-standings.mjs';
import { readTable, recordEntry, tablePath, writeTable } from '../../Gauntlet/Harness/table.mjs';
import { gauntletDetail, gauntletRunText, gauntletStandingsText, projectGauntletStandings, renderGauntletDetail, renderGauntletStandings } from '../App/UI/gauntlet.js';

const FIXTURE = fileURLToPath(new URL('./Fixtures/gauntlet-results.json', import.meta.url));
const PEERS_FIXTURE = fileURLToPath(new URL('./Fixtures/gauntlet-results-peers.json', import.meta.url));
const report = (file = FIXTURE) => JSON.parse(readFileSync(file, 'utf8'));
const TABLE = { updated: '2026-11-14T12:00:00.000Z', entries: 1, rows: 9 };
const view = (file = FIXTURE) => projectGauntletStandings({ file: 'table.json', table: TABLE, standings: report(file).standings });
const row = (projected, peer) => projected.entries[0].rows.find((item) => item.peer === peer);
const cell = (projected, peer, metric) => row(projected, peer).cells.find((item) => item.metric === metric);
const panel = (projected, metric) => projected.entries[0].panels.find((item) => item.metric === metric);
const bars = (projected, metric) => panel(projected, metric).bars.map((bar) => `${bar.label} ${bar.display}`);

// The merged fixture as the peers-only run before it: every Jet value gone.
function withoutJet(standings) {
  const reason = 'not measured: this run measures rust, c, zig, go, python, node, bun only';
  for (const entry of standings.entries) {
    entry.jet = { status: 'not_measured', reason };
    for (const item of Object.values(entry.peers)) {
      if (item.status !== 'ok') continue;
      for (const metric of Object.values(item.metrics)) Object.assign(metric, { jet: null, ratio: null, verdict: 'unmeasured', reason: `Jet: ${reason}` });
    }
  }
  return standings;
}

test('every entry gets a row for every peer and the asm ceiling, every metric a cell', () => {
  const projected = view();
  assert.deepEqual(projected.peers.map((peer) => peer.peer), ['rust', 'c', 'zig', 'go', 'python', 'node', 'bun', 'lua', 'luajit', 'asm']);
  assert.deepEqual(projected.columns.map((column) => column.label), ['Jet build', 'Rust', 'C', 'Zig', 'Go', 'Python', 'Node', 'Bun', 'Lua', 'LuaJIT', 'asm ceiling', 'Jet run'], 'a results file names no Jet modes, so every peer is judged against jet build');
  assert.deepEqual(projected.metrics.map((metric) => metric.id), ['startup', 'wall', 'cycles', 'peak_rss', 'size']);
  for (const item of projected.entries[0].rows) assert.equal(item.cells.length, 5);
  assert.deepEqual(projected.summary, { floor: { pass: 29, fail: 1, other: 5 }, ceiling: { meet: 0, miss: 5, other: 0 } });
});

test('the grid gives every entry a cell for Jet, every peer and the asm ceiling', () => {
  const projected = view();
  const grid = Object.fromEntries(projected.entries[0].grid.filter((cell) => cell.mode === 'build').map((cell) => [cell.peer, cell.text]));
  assert.deepEqual(grid, {
    jet: '417 µs · 292 kB · 57.8k cyc · 95.6 kB',
    rust: '731 µs · 2.45 MB · 565k cyc · 3.26 MB · 5 pass',
    c: '851 µs · 1.95 MB · 617k cyc · 13.4 kB · 4 pass · 1 fail',
    zig: '1.39 ms · 1.26 MB · 481k cyc · 234 kB · 5 pass',
    go: 'uncovered: no go program: go/main.go is missing',
    python: '13.2 ms · 13.3 MB · 44.5M cyc · 11.5 MB · 5 pass',
    node: '27.2 ms · 46.6 MB · 72.9M cyc · 138 MB · 5 pass',
    bun: '15.6 ms · 37.4 MB · 46.6M cyc · 105 MB · 5 pass',
    lua: 'not measured yet: this run\'s standings do not include Lua',
    luajit: 'not measured yet: this run\'s standings do not include LuaJIT',
    asm: '374 µs · 72 kB · 9.63k cyc · 1.45 kB · 5 miss',
  });
  assert.deepEqual(Object.fromEntries(projected.entries[0].grid.filter((cell) => cell.mode === 'build').map((cell) => [cell.peer, cell.tone])),
    { jet: 'jet', rust: 'pass', c: 'fail', zig: 'pass', go: 'na', python: 'pass', node: 'pass', bun: 'pass', lua: 'open', luajit: 'open', asm: 'fail' });
  const html = renderGauntletStandings(projected);
  assert.match(html, /<div class="gtoggle" role="tablist" aria-label="Jet mode"><button type="button" role="tab" class="gtoggle__btn gtoggle__btn--on" aria-selected="true" data-gview="build"[^>]*><b>Compiled<\/b> <small>jet build vs Rust, C, Zig, Go, Python, Node, Bun, Lua, LuaJIT, asm ceiling<\/small><\/button><button type="button" role="tab" class="gtoggle__btn" aria-selected="false" data-gview="run"[^>]*><b>Scripting<\/b>/);
  assert.match(html, /<thead><tr><th scope="col">entry<\/th><th scope="col" class="gcell__jet">Jet build<\/th><th scope="col">Rust<\/th>.*<th scope="col" class="gcol--ceiling">asm ceiling<\/th><\/tr><\/thead>/);
  assert.match(html, /data-gpeer="jet"><ul class="gjet"><li><span class="gbar__name">startup<\/span><b>331 µs<\/b><\/li><li><span class="gbar__name">run time<\/span><b>417 µs<\/b><\/li><li><span class="gbar__name">memory<\/span><b>292 kB<\/b><\/li><\/ul><\/td>/);
  assert.match(html, /<td class="gcell gcell--fail" title="hello · C\nstartup: Jet 331 µs vs 683 µs \(Jet\/peer 0\.48, pass\)\n[^"]*click for details" tabindex="0" role="button" data-gentry="hello" data-gmode="build" data-gpeer="c"><ul class="gbars"><li class="gbar gbar--win"><span class="gbar__name">startup<\/span><span class="gbar__track"><span class="gbar__half gbar__half--win"><span class="gbar__fill" style="width:26\.1%"><\/span><\/span><span class="gbar__half gbar__half--loss"><\/span><\/span><span class="gbar__text">2\.06× faster<\/span><\/li>/);
  assert.match(html, /<td class="gcell gcell--open" title="hello · Lua: not measured yet: this run&#39;s standings do not include Lua" tabindex="0" role="button" data-gentry="hello" data-gmode="build" data-gpeer="lua"><b>not measured yet<\/b><small>this run&#39;s standings do not include Lua<\/small><\/td>/);
  assert.doesNotMatch(html, /gbar__name">compile</, 'a results file without compile times draws no compile line');
  assert.match(html, /<\/table><\/div>\s*<section class="gdetail" data-gdetail aria-live="polite" hidden><\/section>/);
});

test('a peer that beats the asm ceiling flags it; a cell opens its detail with Jet against it on every metric', () => {
  const standings = report().standings;
  standings.entries[0].peers.c.metrics.peak_rss.peer = 40;
  const projected = projectGauntletStandings({ file: 'table.json', table: TABLE, standings });
  const asm = projected.entries[0].grid.find((item) => item.peer === 'asm');
  assert.deepEqual([asm.tone, asm.note, asm.text], ['flag', 'beaten: C peak rss 0.56×', 'ceiling beaten by C on peak rss']);
  assert.deepEqual(projected.entries[0].beaten.map((item) => [item.metric, item.label]), [['peak_rss', 'C']]);
  const flagged = gauntletDetail(projected, 'hello', 'asm');
  assert.equal(flagged.metrics.find((item) => item.metric === 'peak_rss').beaten.label, 'C');
  assert.match(renderGauntletDetail(flagged), /<tr class="gdetail__row--flag"><th scope="row" title="[^"]+">peak RSS<\/th><td>72 kB<span class="gdetail__flag">beaten by C 40 kB \(0\.56×\): the reference is wrong<\/span>/);

  const rust = gauntletDetail(view(), 'hello', 'rust');
  assert.deepEqual([rust.label, rust.role, rust.status, rust.jetMeasured], ['Rust', 'peer', 'ok', true]);
  const startup = rust.metrics.find((item) => item.metric === 'startup');
  assert.equal(startup.versus.tone, 'pass');
  assert.match(startup.versus.words, /^Jet \S+× faster \(0\.54\)$/);
  const size = gauntletDetail(view(), 'hello', 'c').metrics.find((item) => item.metric === 'size');
  assert.deepEqual([size.versus.tone, size.versus.words], ['fail', 'Jet 7.16× larger (+616%)']);
  const html = renderGauntletDetail(gauntletDetail(view(), 'hello', 'c'));
  assert.match(html, /^<header class="gdetail__head"><h3>hello <small>batch<\/small> · C<\/h3><button type="button" class="gdetail__close" data-gclose aria-label="Close details">×<\/button><\/header>/);
  assert.match(html, /<thead><tr><th scope="col">metric<\/th><th scope="col">C<\/th><th scope="col">Jet build<\/th><th scope="col">Jet build vs C<\/th><th scope="col">best peer<\/th><th scope="col">asm ceiling<\/th><\/tr><\/thead>/);
  assert.match(html, /<span class="gdetail__vs gdetail__vs--fail">Jet 7\.16× larger \(\+616%\)<\/span>/);
  const jet = gauntletDetail(view(), 'hello', 'jet');
  assert.deepEqual([jet.role, jet.status], ['jet', 'ok']);
  assert.ok(jet.metrics.every((item) => item.ceiling && item.ceiling.versus), 'Jet is set against the asm ceiling on every metric');
  const pending = gauntletDetail(view(PEERS_FIXTURE), 'hello', 'rust');
  assert.ok(pending.metrics.every((item) => item.jet === null && item.versus === null));
  assert.match(renderGauntletDetail(pending), /<span class="gdetail__missing">not measured yet<\/span>/);
  assert.equal(gauntletDetail(view(), 'no-such-entry', 'rust'), null);
  assert.equal(renderGauntletDetail(null), '');
});

test('ratios carry the floor verdict and the ceiling gap; non-measured rows keep their reason', () => {
  const projected = view();
  assert.equal(cell(projected, 'c', 'size').text, '7.16 fail');
  assert.equal(cell(projected, 'rust', 'startup').text, '0.54 pass');
  assert.equal(cell(projected, 'asm', 'startup').text, '1.12 miss +12%');
  assert.equal(row(projected, 'asm').role, 'ceiling');
  assert.match(cell(projected, 'python', 'size').detail, /peer script\+runtime/);
  assert.equal(cell(projected, 'c', 'size').peerBasis, 'static stripped');
  assert.equal(cell(projected, 'c', 'size').jetBasis, 'static stripped');
  assert.match(cell(projected, 'rust', 'size').detail, /\(Jet static stripped, peer stripped \+ shared libraries: \S+ kB binary \+ \S+ MB in 3 shared libraries\)$/);
  assert.deepEqual(projected.sizeBasis.map((item) => item.basis), ['static stripped', 'stripped + shared libraries', 'web artifact', 'script+runtime']);
  assert.equal(row(projected, 'go').status, 'uncovered');
  assert.equal(row(projected, 'go').reason, 'no go program: go/main.go is missing');
  assert.equal(row(projected, 'vite'), undefined, 'the gauntlet has no Vite peer');
});

test('a peer missing from the results file stays a row with a reason', () => {
  const standings = report().standings;
  delete standings.entries[0].peers.zig;
  const projected = projectGauntletStandings({ standings });
  assert.equal(row(projected, 'zig').status, 'failed');
  assert.match(row(projected, 'zig').reason, /no row for this peer/);
  assert.equal(projectGauntletStandings({ standings: { schema: 'other' } }), null);
  const { size_basis: _unstated, ...older } = report().standings;
  assert.match(gauntletStandingsText(projectGauntletStandings({ standings: older })), /\nsize basis: not stated by this results file/);
});

test('a peers-only run shows every peer value as a bar, the asm ceiling marked, and Jet pending', () => {
  const projected = view(PEERS_FIXTURE);
  assert.equal(projected.jetMeasured, false);
  assert.deepEqual(projected.entries[0].jet, { status: 'not_measured', reason: 'not measured: this run measures rust, c, zig, go, python, node, bun, lua, luajit only', measured: false, provenance: null, runs: null });
  assert.deepEqual(projected.entries[0].grid.filter((cell) => ['rust', 'c', 'python', 'bun', 'asm'].includes(cell.peer)).map((cell) => cell.tone), ['gray', 'gray', 'gray', 'gray', 'gray'], 'until Jet is measured every measured cell is gray');
  assert.deepEqual(bars(projected, 'startup'), ['asm ceiling 296 µs', 'Rust 614 µs', 'C 683 µs', 'Bun 7.31 ms', 'Python 18.2 ms']);
  assert.deepEqual(bars(projected, 'size'), ['asm ceiling 1.45 kB', 'C 13.4 kB', 'Rust 319 kB', 'Python 6.21 MB']);
  assert.deepEqual(panel(projected, 'startup').bars.map((bar) => bar.role), ['ceiling', 'peer', 'peer', 'peer', 'peer']);
  assert.ok(projected.entries[0].panels.every((item) => !item.jet && item.bars.every((bar) => bar.ratio === null && bar.text === null)));
  assert.deepEqual(panel(projected, 'size').missing, [{ peer: 'bun', label: 'Bun', role: 'peer', status: 'unmeasured', reason: 'bun runtime footprint unmeasured: ldd exit 1; Jet: not measured: this run measures rust, c, zig, go, python, node, bun, lua, luajit only' }]);
  assert.deepEqual(projected.entries[0].rows.filter((item) => item.status !== 'ok').map((item) => `${item.peer} ${item.status}: ${item.reason}`), [
    'zig unavailable: cold build exit 127',
    'go uncovered: no go program: go/main.go is missing',
    'node mismatched: line 1 differs: expected `hello, world`, got `hello world`',
    'lua not_measured: this run\'s standings do not include Lua',
    'luajit not_measured: this run\'s standings do not include LuaJIT',
  ]);

  const text = gauntletStandingsText(projected);
  assert.match(text, /\nJet pending: no Jet row is measured yet, so no ratio or verdict is shown\n/);
  assert.doesNotMatch(text, /\d\.\d\d (pass|fail|meet|miss)|floor \d|\n {2}Jet /);
  assert.match(text, /\nhello \(batch\) · Jet build pending · Jet run pending\n {2}peer +startup +run wall +run cycles +peak RSS +size\n {2}Rust +614 µs +795 µs +702 k cycles +2\.11 MB +319 kB\n/);
  assert.match(text, /\n {2}Zig +unavailable: cold build exit 127\n/);
  assert.match(text, /\n {2}Node +mismatched: line 1 differs: expected `hello, world`, got `hello world`\n/);
  assert.match(text, /\n {2}Bun +7\.31 ms +9\.04 ms +11\.9 M cycles +30\.7 MB +—\n/);
  assert.match(text, /\n {2}asm ceiling +296 µs +372 µs +41 k cycles +256 kB +1\.45 kB\n/);
  assert.match(text, /\n {2}· Bun size: unmeasured: bun runtime footprint unmeasured: ldd exit 1/);

  const html = renderGauntletStandings(projected);
  assert.match(html, /<p class="gscore__pending">Jet pending: no Jet row is measured yet, so there are no wins or losses to show\.<\/p>/);
  assert.doesNotMatch(html, /gbar--win|gbar--loss|gscore__tile|floor pass/);
  assert.match(html, /<td class="gcell gcell__jet gcell--open" tabindex="0" role="button" data-gentry="hello" data-gmode="build" data-gpeer="jet"><b>not measured yet<\/b><small>not measured: this run measures rust, c, zig, go, python, node, bun, lua, luajit only<\/small><\/td>/);
  assert.match(html, /data-gpeer="c"><ul class="gbars"><li class="gbar gbar--none"><span class="gbar__name">startup<\/span>.*?<span class="gbar__text">683 µs<\/span><\/li>/, 'before Jet is measured a peer cell shows the peer\'s own values');
  assert.match(html, /data-gpeer="asm"><ul class="gbars"><li class="gbar gbar--none"><span class="gbar__name">startup<\/span>.*?<span class="gbar__text">296 µs<\/span>/);
  assert.match(html, /<td class="gcell gcell--fail" title="hello · Zig: unavailable: cold build exit 127"[^>]*><b>unavailable<\/b><small>cold build exit 127<\/small><\/td>/);
  assert.match(html, /<td class="gcell gcell--na" title="hello · Go: uncovered: no go program: go\/main\.go is missing"/);
  assert.match(html, /<td class="gcell gcell--fail" title="hello · Node: mismatched: line 1 differs/);
});

test('Jet pending names why only when Jet was run and did not count', () => {
  const older = withoutJet(report().standings);
  older.entries[0].jet = { status: 'failed', reason: 'not measured: this run measures rust only' };
  assert.match(gauntletStandingsText(projectGauntletStandings({ standings: older })), /\nhello \(batch\) · Jet build pending · Jet run pending\n/);
  const broken = withoutJet(report().standings);
  broken.entries[0].jet = { status: 'failed', reason: 'cold build exit 1' };
  const projected = projectGauntletStandings({ standings: broken });
  assert.match(gauntletStandingsText(projected), /\nhello \(batch\) · Jet build pending \(failed: cold build exit 1\) · Jet run pending\n/);
  assert.match(renderGauntletStandings(projected), /data-gpeer="jet"><b>failed<\/b><small>cold build exit 1<\/small><\/td>/);
});

test('a run with Jet merged draws Jet beside the same peer bars, with ratios and verdicts', () => {
  const projected = view();
  const pending = projectGauntletStandings({ standings: withoutJet(report().standings) });
  assert.equal(projected.jetMeasured, true);
  assert.equal(pending.jetMeasured, false);
  for (const metric of projected.metrics) {
    assert.deepEqual(bars(projected, metric.id).filter((bar) => !bar.startsWith('Jet ')), bars(pending, metric.id));
  }
  assert.deepEqual(bars(projected, 'size'), ['asm ceiling 1.45 kB', 'C 13.4 kB', 'Jet build 95.6 kB', 'Zig 234 kB', 'Rust 3.26 MB', 'Python 11.5 MB', 'Bun 105 MB', 'Node 138 MB']);
  const c = panel(projected, 'size').bars.find((bar) => bar.peer === 'c');
  assert.deepEqual([c.ratio, c.verdict, c.text], [7.156, 'fail', '7.16 fail']);
  assert.equal(panel(projected, 'startup').bars.find((bar) => bar.role === 'ceiling').text, '1.12 miss +12%');

  const text = gauntletStandingsText(projected);
  assert.match(text, /^Gauntlet · table table\.json · 9 rows over 1 entries · updated 2026-11-14 12:00 UTC\n/);
  assert.match(text, /floor 29 pass, 1 fail, 5 other · ceiling 0 meet, 5 miss, 0 other/);
  assert.match(text, /\nsize basis \(each size value names its basis\):\n {2}static stripped: Jet, Rust, C, Zig, Go and asm/);
  assert.match(text, /\nhello \(batch\) · Jet run pending\n {2}peer +startup +run wall +run cycles +peak RSS +size\n {2}Jet build +331 µs +417 µs +57\.8 k cycles +292 kB +95\.6 kB\n/);
  assert.match(text, /\n {2}C +683 µs 0\.48 pass +851 µs 0\.49 pass +617 k cycles 0\.09 pass +1\.95 MB 0\.15 pass +13\.4 kB 7\.16 fail\n/);
  assert.match(text, /\n {2}size basis: Jet static stripped · Rust stripped \+ shared libraries · C static stripped · Zig static stripped · Python script\+runtime · Node script\+runtime · Bun script\+runtime · asm ceiling static stripped(\n|$)/);
  assert.match(text, /\n {2}Go +uncovered: no go program: go\/main\.go is missing\n/);
  assert.match(text, /\n {2}asm ceiling +296 µs 1\.12 miss \+12% +374 µs 1\.12 miss \+12% +9\.63 k cycles 6\.01 miss \+501% +72 kB 4\.06 miss \+306% +1\.45 kB 65\.76 miss \+6476%/);

  const html = renderGauntletStandings(projected);
  assert.doesNotMatch(html, /gscore__pending/);
  assert.match(html, /<div class="gscore__tile gscore__tile--win"><h3>startup<\/h3><p class="gscore__count"><b class="gscore__win">6<\/b> won <b class="gscore__loss">0<\/b> lost <span>2 open<\/span><\/p><p class="gscore__ceil">asm ceiling: 0 met, 1 missed<\/p><\/div>/);
  assert.match(html, /<h3>memory<\/h3>/);
  assert.match(html, /<div class="glegend" aria-hidden="true">.*<i style="left:37\.5%">2×<\/i><i style="left:50%">even<\/i><i style="left:62\.5%">2×<\/i>/);
  assert.match(html, /data-gpeer="asm"><ul class="gbars"><li class="gbar gbar--gap"><span class="gbar__name">startup<\/span>.*?<span class="gbar__half gbar__half--loss"><span class="gbar__fill" style="width:4%"><\/span><\/span><\/span><span class="gbar__text">1\.12× slower<\/span>/, 'a missed ceiling is a gap, not a gated loss');
  assert.match(html, /<details class="gauntlet__rules"><summary>Rules and size basis<\/summary>.*floor 29 pass, 1 fail, 5 other/s);
  assert.match(html, /<p class="gauntlet__policy standings__basis"><strong>size basis \(each size value names its basis\):<\/strong><span>static stripped: /);
  assert.match(html, /<td class="gcell gcell--na" title="hello · Go: uncovered: no go program: go\/main\.go is missing"/);
});

test('the page shows the gauntlet table: each row it holds, every other cell not measured yet or why not', async () => {
  const root = mkdtempSync(join(tmpdir(), 'tower-standings-'));
  try {
    const file = join(root, 'state', 'table.json');
    const empty = await readGauntletStandings(root, file);
    assert.deepEqual([empty.file, empty.table], [join('state', 'table.json'), { updated: null, entries: 0, rows: 0 }]);
    assert.equal(projectGauntletStandings(empty).planned, true);
    const table = await readTable(file);
    recordEntry(table, { entry: { name: 'hello' }, rows: { jet: { status: 'broken', reason: 'jet row reason' } }, standings_rows: { rust: { status: 'broken', reason: 'rust row reason' }, c: { not_measured: true } } },
      { runId: 'run-1', generated: '2026-11-14T12:00:00.000Z', commit: 'abc', toolchains: { rust: { version: 'rustc 1' } }, jetBinarySha256: 'jet-sha', programs: { rust: 'rust-sha' } });
    assert.deepEqual(Object.keys(table.rows.hello).sort(), ['jet', 'rust'], 'a row the run did not measure stays out');
    assert.deepEqual(table.rows.hello.rust.measured, { at: '2026-11-14T12:00:00.000Z', run_id: 'run-1', commit: 'abc', toolchain: 'rustc 1', program_sha256: 'rust-sha' });
    await writeTable(table, file);
    const shown = await readGauntletStandings(root, file);
    assert.deepEqual(shown.table, { updated: '2026-11-14T12:00:00.000Z', entries: 1, rows: 2 });
    const projected = projectGauntletStandings(shown);
    assert.equal(projected.planned, false);
    assert.equal(gauntletRunText(projected), 'table state/table.json · 2 rows over 1 entries · updated 2026-11-14 12:00 UTC');
    const hello = Object.fromEntries(projected.entries.find((entry) => entry.entry === 'hello').grid.map((cell) => [cell.peer, cell.text]));
    assert.equal(hello.rust, 'failed: rust row reason');
    assert.equal(hello.c, 'not measured yet');
    writeFileSync(file, '{');
    await assert.rejects(readGauntletStandings(root, file), (error) => error instanceof TowerError && error.code === 'E_INVALID');
    const state = process.env.JET_STATE_ROOT;
    process.env.JET_STATE_ROOT = join(root, 'state');
    try {
      assert.equal(tablePath(), join(root, 'state', 'gauntlet', 'table.json'));
    } finally {
      if (state === undefined) delete process.env.JET_STATE_ROOT;
      else process.env.JET_STATE_ROOT = state;
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('the planned grid covers every entry × Jet, every peer and asm, each cell not measured yet or why not', async () => {
  const projected = projectGauntletStandings({ file: null, table: { updated: null, entries: 0, rows: 0 }, standings: await tableStandings({ rows: {} }) });
  assert.equal(projected.planned, true);
  assert.equal(projected.jetMeasured, false);
  assert.deepEqual(projected.columns.map((column) => `${column.mode}:${column.peer}`), ['build:jet', 'build:rust', 'build:c', 'build:zig', 'build:go', 'build:asm', 'run:jet', 'run:python', 'run:node', 'run:bun', 'run:lua', 'run:luajit']);
  assert.ok(projected.entries.length > 20);
  for (const entry of projected.entries) {
    assert.equal(entry.grid.length, 12, entry.entry);
    assert.deepEqual(entry.grid[0], { peer: 'jet', mode: 'build', label: 'Jet build', head: 'not measured yet', note: null, text: 'not measured yet', tone: 'open', reason: 'not measured yet' });
    assert.deepEqual(entry.grid[6], { peer: 'jet', mode: 'run', label: 'Jet run', head: 'not measured yet', note: null, text: 'not measured yet', tone: 'open', reason: 'not measured yet' });
    for (const cell of entry.grid) assert.ok(['open', 'na', 'fail'].includes(cell.tone), `${entry.entry} ${cell.peer}: ${cell.text}`);
  }
  const cells = projected.entries.flatMap((entry) => entry.grid.map((cell) => ({ entry: entry.entry, ...cell })));
  const hello = Object.fromEntries(projected.entries.find((entry) => entry.entry === 'hello').grid.map((cell) => [cell.peer, cell.text]));
  assert.equal(hello.rust, 'not measured yet');
  assert.equal(hello.lua, 'not measured yet');
  assert.equal(hello.asm, 'not measured yet');
  for (const cell of cells.filter((item) => item.peer === 'lua' && item.tone === 'na' && /^uncovered/.test(item.text))) assert.equal(cell.text, 'uncovered: no lua program: Lua/main.lua is missing', cell.entry);
  const bare = cells.filter((cell) => cell.peer === 'asm' && cell.tone === 'na');
  assert.ok(bare.length > 0, 'an entry with no asm reference keeps its asm cell');
  for (const cell of bare) assert.equal(cell.text, 'uncovered: no ceiling reference');

  const text = gauntletStandingsText(projected);
  assert.equal(gauntletRunText(projected), 'gauntlet table: nothing measured yet, every measurable cell is not measured yet');
  assert.match(text, /^Gauntlet · gauntlet table: nothing measured yet/);
  assert.match(text, /\nhello \(batch\)\n {2}Jet build +not measured yet\n {2}Rust +not measured yet\n(.*\n)*? {2}asm ceiling +[^\n]+\n {2}Jet run +not measured yet\n {2}Python +not measured yet\n/);
  const html = renderGauntletStandings(projected);
  assert.match(html, /^<section class="standings" aria-label="Gauntlet standings" data-gshown="build"><p class="gauntlet__run">gauntlet table: nothing measured yet/);
  assert.match(html, /<td class="gcell gcell--na" title="[^"]+ · asm ceiling: uncovered: no ceiling reference \([^"]+\)" tabindex="0" role="button" data-gentry="[^"]+" data-gmode="build" data-gpeer="asm"><b>uncovered<\/b><small>no ceiling reference<\/small><\/td>/);
  assert.doesNotMatch(html, /data-gmode="run"/, 'the Compiled page shows jet build and its peers only');
  const scripting = renderGauntletStandings(projected, 'run');
  assert.match(scripting, /data-gmode="run" data-gpeer="python"/);
  assert.doesNotMatch(scripting, /data-gpeer="(asm|rust|c|zig|go)"/);
  assert.doesNotMatch(html + scripting, /gscore|glegend|gauntlet__rules/, 'nothing measured: no tally, legend or rules');
});

test('`tower gauntlet standings` prints the gauntlet table', async () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-standings-cli-'));
  const cliModule = new URL('../App/cli.mjs', import.meta.url).href;
  const cli = (...args) => spawnSync(process.execPath, ['--input-type=module', '--eval',
    `const { run } = await import(${JSON.stringify(cliModule)}); await run(process.argv.slice(1));`,
    '--', ...args], { cwd, encoding: 'utf8', maxBuffer: 1 << 26, env: { ...process.env, TOWER_DATA: '', JET_STATE_ROOT: join(cwd, 'state') } });
  try {
    mkdirSync(join(cwd, '.tower'));
    writeJSON(join(cwd, '.tower', 'tower.json'), empty('Standings CLI'));
    const planned = cli('gauntlet', 'standings');
    assert.equal(planned.status, 0, planned.stderr);
    assert.match(planned.stdout, /^Gauntlet · table state\/gauntlet\/table\.json: nothing measured yet/);
    assert.match(planned.stdout, /\nhello \(batch\)\n {2}Jet build +not measured yet\n {2}Rust +not measured yet\n/);
    assert.match(planned.stdout, /\n {2}Jet run +not measured yet\n {2}Python +not measured yet\n/);
    const file = join(cwd, 'other.json');
    const table = await readTable(file);
    recordEntry(table, { entry: { name: 'hello' }, rows: {}, standings_rows: { rust: { status: 'broken', reason: 'rust row reason' } } }, { runId: 'r', generated: '2026-05-15T08:00:00.000Z', toolchains: {} });
    await writeTable(table, file);
    const shown = cli('gauntlet', 'standings', '--table', 'other.json');
    assert.equal(shown.status, 0, shown.stderr);
    assert.ok(shown.stdout.length < 200_000, `the text table is ${shown.stdout.length} bytes`);
    assert.match(shown.stdout, /^Gauntlet · table other\.json · 1 rows over 1 entries · updated 2026-05-15 08:00 UTC\n/);
    const json = cli('gauntlet', 'standings', '--table', 'other.json', '--json');
    assert.equal(json.status, 0, json.stderr);
    assert.equal(JSON.parse(json.stdout).entries.find((entry) => entry.entry === 'hello').rows.length, 10);
    const help = cli('help', '--check');
    assert.equal(help.status, 0, help.stderr + help.stdout);
  } finally {
    rmSync(cwd, { recursive: true, force: true });
  }
});

test('one line per entry: compiled peers are judged against jet build\'s binary, interpreted peers against jet run', async () => {
  const metrics = (wall, rss, cycles) => ({ runtime_wall_seconds: wall, runtime_first_stdout_seconds: wall, runtime_peak_rss_kb: rss, runtime_cycles: cycles, binary_bytes: 100_000 });
  const sample = (cycles) => ({ wall_seconds: 2, peak_rss_kb: 900_000, cycles_u: cycles });
  const table = { rows: {}, runtimes: {} };
  recordEntry(table, {
    entry: { name: 'hello' },
    rows: { jet: { status: 'ok', metrics: metrics(0.0004, 300, 60_000), size: { status: 'ok', bytes: 100_000, basis: 'static stripped' } } },
    standings_rows: {
      c: { status: 'ok', metrics: { ...metrics(0.0008, 2_000, 600_000), binary_bytes: 200_000 }, size: { status: 'ok', bytes: 200_000, basis: 'static stripped' } },
      python: { status: 'ok', metrics: metrics(0.05, 10_000, 40e6), size: { status: 'ok', bytes: 5_000_000, basis: 'script+runtime' } },
    },
    jet_tiers: { run: { status: 'ok', command: ['jet', 'run', 'run.jet'], metrics: { runtime_wall_seconds: 2, runtime_first_stdout_seconds: 2, runtime_peak_rss_kb: 900_000 }, runtime: { samples: [sample(9e9), sample(7e9), sample(8e9)] } } },
  }, { runId: 'r', generated: '2026-11-14T12:00:00.000Z', toolchains: {}, jetBinarySha256: 'jet-sha', programs: { jet: 'jet-program' } });
  const run = table.rows.hello['jet-run'];
  assert.equal(run.metrics.runtime_cycles, 8e9, 'jet run cycles are the samples\' median');
  assert.equal(run.size.status, 'not_applicable');
  assert.deepEqual([run.measured.toolchain, run.measured.program_sha256], [table.rows.hello.jet.measured.toolchain, 'jet-program']);
  const unmeasured = { rows: {} };
  recordEntry(unmeasured, { entry: { name: 'hello' }, rows: { jet: { status: 'ok', metrics: {} } }, jet_tiers: { run: { status: 'not_measured' } } }, { runId: 'r', generated: 'g', toolchains: {} });
  assert.equal(unmeasured.rows.hello['jet-run'], undefined, 'an unmeasured run tier writes no row');

  const standings = await tableStandings(table);
  assert.deepEqual([standings.jet_modes.c, standings.jet_modes.asm, standings.jet_modes.python, standings.jet_modes.luajit], ['build', 'build', 'run', 'run']);
  assert.equal(standings.run, undefined, 'one judgment, no second section');
  const judged = standings.entries.find((entry) => entry.entry === 'hello');
  assert.equal(judged.peers.python.metrics.wall.jet, 2, 'Python is judged against jet run');
  assert.equal(judged.peers.c.metrics.wall.jet, 0.0004, 'C is judged against jet build\'s binary');
  assert.equal(judged.jet_run.runs, 3);

  const projected = projectGauntletStandings({ file: 'table.json', table: { updated: 'g', entries: 1, rows: 4 }, standings });
  const hello = projected.entries.find((entry) => entry.entry === 'hello');
  assert.deepEqual([projected.jetMeasured, hello.jets.build.measured, hello.jets.run.measured], [true, true, true]);
  const cellOf = (mode, peer) => hello.grid.find((item) => item.mode === mode && item.peer === peer);
  assert.deepEqual([cellOf('build', 'c').tone, cellOf('run', 'python').tone], ['pass', 'fail'], 'the built binary beats C; jet run loses to Python');
  assert.deepEqual([cellOf('build', 'jet').label, cellOf('run', 'jet').label], ['Jet build', 'Jet run']);
  assert.equal(cellOf('run', 'c'), undefined, 'each peer has one cell');

  const python = gauntletDetail(projected, 'hello', 'python', 'build');
  assert.deepEqual([python.jetMode, python.jetLabel, python.hasCeiling], ['run', 'Jet run', false], 'a peer\'s own mode decides');
  assert.equal(python.metrics.find((item) => item.metric === 'wall').versus.tone, 'fail');
  assert.equal(python.metrics.find((item) => item.metric === 'size').jet, null, 'jet run writes no binary');
  assert.match(renderGauntletDetail(python), /<thead><tr><th scope="col">metric<\/th><th scope="col">Python<\/th><th scope="col">Jet run<\/th><th scope="col">Jet run vs Python<\/th><th scope="col">best peer<\/th><\/tr><\/thead>/);
  const c = gauntletDetail(projected, 'hello', 'c');
  assert.deepEqual([c.jetMode, c.metrics.find((item) => item.metric === 'wall').versus.tone], ['build', 'pass']);
  assert.match(renderGauntletDetail(c), /<th scope="col">C<\/th><th scope="col">Jet build<\/th><th scope="col">Jet build vs C<\/th><th scope="col">best peer<\/th><th scope="col">asm ceiling<\/th>/);
  assert.equal(gauntletDetail(projected, 'hello', 'jet', 'run').label, 'Jet run');

  const page = renderGauntletStandings(projected);
  assert.equal(page.match(/<tr><th scope="row"><div><strong>hello<\/strong>/g).length, 1, 'one grid line per entry');
  assert.match(page, /<b>Compiled<\/b> <small>jet build vs Rust, C, Zig, Go, asm ceiling<\/small>/);
  assert.match(page, /<b>Scripting<\/b> <small>jet run vs Python, Node, Bun, Lua, LuaJIT<\/small>/);
  assert.match(page, /data-gentry="hello" data-gmode="build" data-gpeer="c"><ul class="gbars"><li class="gbar gbar--none"><span class="gbar__name">compile<\/span>/);
  assert.match(page, /<h3>compile<\/h3>/);
  assert.doesNotMatch(page, /data-gmode="run"/);
  const scripting = renderGauntletStandings(projected, 'run');
  assert.match(scripting, /data-gentry="hello" data-gmode="run" data-gpeer="python"><ul class="gbars"><li class="gbar gbar--loss"><span class="gbar__name">startup \+ compile<\/span>/);
  assert.match(scripting, /data-gpeer="jet"><ul class="gjet"><li><span class="gbar__name">startup \+ compile<\/span><b>2 s<\/b><\/li>/);
  assert.doesNotMatch(scripting, /gbar__name">compile<|<h3>compile<\/h3>/, 'jet run compiles inside startup: no compile line');
  assert.equal(renderGauntletStandings(projected, 'nonsense'), page, 'an unknown mode shows Compiled');
  const text = gauntletStandingsText(projected);
  assert.match(text, /\nJet build judges Rust, C, Zig, Go, asm ceiling: /);
  assert.match(text, /\nJet run judges Python, Node, Bun, Lua, LuaJIT: /);
  assert.match(text, /\n {2}peer +compile +startup +run wall/, 'compile time is the first column');
  assert.match(text, /\n {2}Jet build +— +\S+ µs[^\n]*\n(.*\n)*? {2}C +— +\S+ µs \d\.\d\d pass[^\n]*\n(.*\n)*? {2}Jet run +— +2 s[^\n]*\n {2}Python +— +\S+ ms \d+\.\d\d fail/);
  assert.doesNotMatch(text, /· run \d/);
});
