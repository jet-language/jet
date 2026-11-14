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
import { empty, TowerError } from '../app/store.mjs';
import { writeJSON } from '../app/paths.mjs';
import { readGauntletStandings, tableStandings } from '../app/gauntlet-standings.mjs';
import { readTable, recordEntry, tablePath, writeTable } from '../../gauntlet/harness/table.mjs';
import { gauntletRunText, gauntletStandingsText, projectGauntletStandings, renderGauntletStandings } from '../app/ui/gauntlet.js';

const FIXTURE = fileURLToPath(new URL('./fixtures/gauntlet-results.json', import.meta.url));
const PEERS_FIXTURE = fileURLToPath(new URL('./fixtures/gauntlet-results-peers.json', import.meta.url));
const report = (file = FIXTURE) => JSON.parse(readFileSync(file, 'utf8'));
const TABLE = { updated: '2026-11-14T12:00:00.000Z', entries: 1, rows: 9 };
const view = (file = FIXTURE) => projectGauntletStandings({ file: 'table.json', table: TABLE, standings: report(file).standings });
const row = (projected, peer) => projected.entries[0].rows.find((item) => item.peer === peer);
const cell = (projected, peer, metric) => row(projected, peer).cells.find((item) => item.metric === metric);
const panel = (projected, metric) => projected.entries[0].panels.find((item) => item.metric === metric);
const bars = (projected, metric) => panel(projected, metric).bars.map((bar) => `${bar.label} ${bar.display}`);

// The merged fixture as the peers-only run before it: every Jet value gone.
function withoutJet(standings) {
  const reason = 'not measured: this run measures rust, c, zig, go, python, node, bun, vite only';
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
  assert.deepEqual(projected.peers.map((peer) => peer.peer), ['rust', 'c', 'zig', 'go', 'python', 'node', 'bun', 'vite', 'lua', 'luajit', 'asm']);
  assert.deepEqual(projected.columns.map((column) => column.label), ['Jet', 'Rust', 'C', 'Zig', 'Go', 'Python', 'Node', 'Bun', 'Vite', 'Lua', 'LuaJIT', 'asm ceiling']);
  assert.deepEqual(projected.metrics.map((metric) => metric.id), ['startup', 'wall', 'cycles', 'peak_rss', 'size']);
  for (const item of projected.entries[0].rows) assert.equal(item.cells.length, 5);
  assert.deepEqual(projected.summary, { floor: { pass: 29, fail: 1, other: 10 }, ceiling: { meet: 0, miss: 5, other: 0 } });
});

test('the grid gives every entry a cell for Jet, every peer and the asm ceiling', () => {
  const projected = view();
  const grid = Object.fromEntries(projected.entries[0].grid.map((cell) => [cell.peer, cell.text]));
  assert.deepEqual(grid, {
    jet: 'measured',
    rust: '5 pass',
    c: '4 pass · 1 fail',
    zig: '5 pass',
    go: 'uncovered: no go program: go/main.go is missing',
    python: '5 pass',
    node: '5 pass',
    bun: '5 pass',
    vite: 'not applicable: Vite bundles web front ends; an entry without a web front end has nothing for it to build',
    lua: 'not measured yet: this run\'s standings do not include Lua',
    luajit: 'not measured yet: this run\'s standings do not include LuaJIT',
    asm: '5 miss',
  });
  const html = renderGauntletStandings(projected);
  assert.match(html, /<table class="gauntlet__table" aria-label="Gauntlet grid">\s*<thead><tr><th scope="col">entry<\/th><th scope="col">Jet<\/th><th scope="col">Rust<\/th>.*<th scope="col">LuaJIT<\/th><th scope="col">asm ceiling<\/th><\/tr><\/thead>/);
  assert.match(html, /<td class="ggrid ggrid--fail" title="hello · C: 4 pass · 1 fail"><b>4 pass · 1 fail<\/b><\/td>/);
  assert.match(html, /<td class="ggrid ggrid--open" title="hello · Lua: not measured yet: this run&#39;s standings do not include Lua"><b>not measured yet<\/b><small>this run&#39;s standings do not include Lua<\/small><\/td>/);
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
  assert.deepEqual(projected.sizeBasis.map((item) => item.basis), ['static stripped', 'stripped + shared libraries', 'web artifact', 'bundle', 'script+runtime']);
  assert.equal(row(projected, 'go').status, 'uncovered');
  assert.equal(row(projected, 'go').reason, 'no go program: go/main.go is missing');
  assert.equal(row(projected, 'vite').status, 'not_applicable');
  assert.equal(cell(projected, 'vite', 'wall').ratio, null);
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
  assert.deepEqual(projected.entries[0].jet, { status: 'not_measured', reason: 'not measured: this run measures rust, c, zig, go, python, node, bun, vite, lua, luajit only', measured: false });
  assert.deepEqual(bars(projected, 'startup'), ['asm ceiling 296 µs', 'Rust 614 µs', 'C 683 µs', 'Bun 7.31 ms', 'Python 18.2 ms']);
  assert.deepEqual(bars(projected, 'size'), ['asm ceiling 1.45 kB', 'C 13.4 kB', 'Rust 319 kB', 'Python 6.21 MB']);
  assert.deepEqual(panel(projected, 'startup').bars.map((bar) => bar.role), ['ceiling', 'peer', 'peer', 'peer', 'peer']);
  assert.ok(projected.entries[0].panels.every((item) => !item.jet && item.bars.every((bar) => bar.ratio === null && bar.text === null)));
  assert.deepEqual(panel(projected, 'size').missing, [{ peer: 'bun', label: 'Bun', role: 'peer', status: 'unmeasured', reason: 'bun runtime footprint unmeasured: ldd exit 1; Jet: not measured: this run measures rust, c, zig, go, python, node, bun, vite, lua, luajit only' }]);
  assert.deepEqual(projected.entries[0].rows.filter((item) => item.status !== 'ok').map((item) => `${item.peer} ${item.status}: ${item.reason}`), [
    'zig unavailable: cold build exit 127',
    'go uncovered: no go program: go/main.go is missing',
    'node mismatched: line 1 differs: expected `hello, world`, got `hello world`',
    'vite not_applicable: Vite bundles web front ends; an entry without a web front end has nothing for it to build',
    'lua not_measured: this run\'s standings do not include Lua',
    'luajit not_measured: this run\'s standings do not include LuaJIT',
  ]);

  const text = gauntletStandingsText(projected);
  assert.match(text, /\nJet pending: no Jet row is measured yet, so no ratio or verdict is shown\n/);
  assert.doesNotMatch(text, /\d\.\d\d (pass|fail|meet|miss)|floor \d|\n {2}Jet /);
  assert.match(text, /\nhello \(batch\) · Jet pending\n {2}peer +startup +run wall +run cycles +peak RSS +size\n {2}Rust +614 µs +795 µs +702 k cycles +2\.11 MB +319 kB\n/);
  assert.match(text, /\n {2}Zig +unavailable: cold build exit 127\n/);
  assert.match(text, /\n {2}Node +mismatched: line 1 differs: expected `hello, world`, got `hello world`\n/);
  assert.match(text, /\n {2}Bun +7\.31 ms +9\.04 ms +11\.9 M cycles +30\.7 MB +—\n/);
  assert.match(text, /\n {2}asm ceiling +296 µs +372 µs +41 k cycles +256 kB +1\.45 kB\n/);
  assert.match(text, /\n {2}· Bun size: unmeasured: bun runtime footprint unmeasured: ldd exit 1/);

  const html = renderGauntletStandings(projected);
  assert.match(html, /<span class="gauntlet__pill standings__pill--open">Jet <b>pending<\/b><\/span>/);
  assert.doesNotMatch(html, /standings__ratio|floor pass/);
  assert.equal(html.match(/<figure class="standings__panel">/g).length, 5);
  assert.equal(html.match(/<li class="standings__bar standings__bar--pending"><span class="standings__name">Jet<\/span><span class="standings__note">Jet pending<\/span><\/li>/g).length, 5);
  assert.match(html, /<li class="standings__bar standings__bar--ceiling" title="hello · asm ceiling 296 µs"><span class="standings__name">asm ceiling<\/span><span class="standings__track"><span class="standings__fill" style="width:8%"><\/span><\/span><span class="standings__value">296 µs<\/span><\/li>/);
  assert.match(html, /<li class="standings__bar standings__bar--peer" title="hello · Python 18\.2 ms">.*?style="width:100%"/);
  assert.match(html, /standings__bar--missing"><span class="standings__name">Bun<\/span><span class="standings__note"><b>unmeasured<\/b> bun runtime footprint unmeasured: ldd exit 1/);
  assert.match(html, /<b>Zig<\/b> <span>unavailable: cold build exit 127<\/span>/);
  assert.match(html, /<b>Go<\/b> <span>uncovered: no go program: go\/main\.go is missing<\/span>/);
  assert.match(html, /<b>Node<\/b> <span>mismatched: line 1 differs/);
});

test('Jet pending names why only when Jet was run and did not count', () => {
  const older = withoutJet(report().standings);
  older.entries[0].jet = { status: 'failed', reason: 'not measured: this run measures rust only' };
  assert.match(gauntletStandingsText(projectGauntletStandings({ standings: older })), /\nhello \(batch\) · Jet pending\n/);
  const broken = withoutJet(report().standings);
  broken.entries[0].jet = { status: 'failed', reason: 'cold build exit 1' };
  const projected = projectGauntletStandings({ standings: broken });
  assert.match(gauntletStandingsText(projected), /\nhello \(batch\) · Jet pending \(failed: cold build exit 1\)\n/);
  assert.match(renderGauntletStandings(projected), /<span class="standings__note">Jet pending \(failed: cold build exit 1\)<\/span>/);
});

test('a run with Jet merged draws Jet beside the same peer bars, with ratios and verdicts', () => {
  const projected = view();
  const pending = projectGauntletStandings({ standings: withoutJet(report().standings) });
  assert.equal(projected.jetMeasured, true);
  assert.equal(pending.jetMeasured, false);
  for (const metric of projected.metrics) {
    assert.deepEqual(bars(projected, metric.id).filter((bar) => !bar.startsWith('Jet ')), bars(pending, metric.id));
  }
  assert.deepEqual(bars(projected, 'size'), ['asm ceiling 1.45 kB', 'C 13.4 kB', 'Jet 95.6 kB', 'Zig 234 kB', 'Rust 3.26 MB', 'Python 11.5 MB', 'Bun 105 MB', 'Node 138 MB']);
  const c = panel(projected, 'size').bars.find((bar) => bar.peer === 'c');
  assert.deepEqual([c.ratio, c.verdict, c.text], [7.156, 'fail', '7.16 fail']);
  assert.equal(panel(projected, 'startup').bars.find((bar) => bar.role === 'ceiling').text, '1.12 miss +12%');

  const text = gauntletStandingsText(projected);
  assert.match(text, /^Gauntlet · table table\.json · 9 rows over 1 entries · updated 2026-11-14 12:00 UTC\n/);
  assert.match(text, /floor 29 pass, 1 fail, 10 other · ceiling 0 meet, 5 miss, 0 other/);
  assert.match(text, /\nsize basis \(each size value names its basis\):\n {2}static stripped: Jet, Rust, C, Zig, Go and asm/);
  assert.match(text, /\nhello \(batch\)\n {2}peer +startup +run wall +run cycles +peak RSS +size\n {2}Jet +331 µs +417 µs +57\.8 k cycles +292 kB +95\.6 kB\n/);
  assert.match(text, /\n {2}C +683 µs 0\.48 pass +851 µs 0\.49 pass +617 k cycles 0\.09 pass +1\.95 MB 0\.15 pass +13\.4 kB 7\.16 fail\n/);
  assert.match(text, /\n {2}size basis: Jet static stripped · Rust stripped \+ shared libraries · C static stripped · Zig static stripped · Python script\+runtime · Node script\+runtime · Bun script\+runtime · asm ceiling static stripped(\n|$)/);
  assert.match(text, /\n {2}Go +uncovered: no go program: go\/main\.go is missing\n/);
  assert.match(text, /\n {2}Vite +not applicable: Vite bundles web front ends/);
  assert.match(text, /\n {2}asm ceiling +296 µs 1\.12 miss \+12% +374 µs 1\.12 miss \+12% +9\.63 k cycles 6\.01 miss \+501% +72 kB 4\.06 miss \+306% +1\.45 kB 65\.76 miss \+6476%/);

  const html = renderGauntletStandings(projected);
  assert.doesNotMatch(html, /standings__bar--pending|Jet <b>pending/);
  assert.match(html, /<span class="gauntlet__pill standings__pill--fail">floor fail <b>1<\/b><\/span>/);
  assert.equal(html.match(/<li class="standings__bar standings__bar--jet"/g).length, 5);
  assert.match(html, /<li class="standings__bar standings__bar--jet" title="hello · Jet 95\.6 kB \(static stripped\)">/);
  assert.match(html, /<li class="standings__bar standings__bar--peer standings__bar--fail" title="hello · C Jet 95\.6 kB \/ 13\.4 kB \(Jet static stripped, peer static stripped\) · 7\.16 fail">.*?<span class="standings__value">13\.4 kB<em>static stripped<\/em><\/span><b class="standings__ratio">7\.16 fail<\/b><\/li>/);
  assert.match(html, /<li class="standings__bar standings__bar--ceiling standings__bar--fail" title="hello · asm ceiling Jet 331 µs \/ 296 µs · 1\.12 miss \+12%">/);
  assert.match(html, /<p class="gauntlet__policy standings__basis"><strong>size basis \(each size value names its basis\):<\/strong><span>static stripped: /);
  assert.match(html, /<b>Go<\/b> <span>uncovered: no go program: go\/main\.go is missing<\/span>/);
  assert.match(html, /<li class="standings__blocked--na"><b>Vite<\/b> <span>not applicable: /);
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
  assert.deepEqual(projected.columns.map((column) => column.peer), ['jet', 'rust', 'c', 'zig', 'go', 'python', 'node', 'bun', 'vite', 'lua', 'luajit', 'asm']);
  assert.ok(projected.entries.length > 20);
  for (const entry of projected.entries) {
    assert.equal(entry.grid.length, 12, entry.entry);
    assert.deepEqual(entry.grid[0], { peer: 'jet', label: 'Jet', head: 'not measured yet', note: null, text: 'not measured yet', tone: 'open', reason: 'not measured yet' });
    for (const cell of entry.grid) assert.ok(['open', 'na', 'fail'].includes(cell.tone), `${entry.entry} ${cell.peer}: ${cell.text}`);
  }
  const cells = projected.entries.flatMap((entry) => entry.grid.map((cell) => ({ entry: entry.entry, ...cell })));
  const hello = Object.fromEntries(projected.entries.find((entry) => entry.entry === 'hello').grid.map((cell) => [cell.peer, cell.text]));
  assert.equal(hello.rust, 'not measured yet');
  assert.equal(hello.lua, 'not measured yet');
  assert.equal(hello.asm, 'not measured yet');
  assert.match(hello.vite, /^not applicable: /);
  assert.ok(cells.some((cell) => cell.peer === 'lua' && cell.text === 'uncovered: no lua program: lua/main.lua is missing'));
  const bare = cells.filter((cell) => cell.peer === 'asm' && cell.tone === 'na');
  assert.ok(bare.length > 0, 'an entry with no asm reference keeps its asm cell');
  for (const cell of bare) assert.equal(cell.text, 'uncovered: no ceiling reference');

  const text = gauntletStandingsText(projected);
  assert.equal(gauntletRunText(projected), 'gauntlet table: nothing measured yet, every measurable cell is not measured yet');
  assert.match(text, /^Gauntlet · gauntlet table: nothing measured yet/);
  assert.match(text, /\nhello \(batch\)\n {2}Jet +not measured yet\n {2}Rust +not measured yet\n/);
  const html = renderGauntletStandings(projected);
  assert.match(html, /^<section class="standings" aria-label="Gauntlet standings"><p class="gauntlet__run">gauntlet table: nothing measured yet/);
  assert.match(html, /<td class="ggrid ggrid--na" title="[^"]+ · asm ceiling: uncovered: no ceiling reference \([^"]+\)"><b>uncovered<\/b><small>no ceiling reference<\/small><\/td>/);
  assert.doesNotMatch(html, /standings__panel|gauntlet__pill/);
});

test('`tower gauntlet standings` prints the gauntlet table', async () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-standings-cli-'));
  const cliModule = new URL('../app/cli.mjs', import.meta.url).href;
  const cli = (...args) => spawnSync(process.execPath, ['--input-type=module', '--eval',
    `const { run } = await import(${JSON.stringify(cliModule)}); await run(process.argv.slice(1));`,
    '--', ...args], { cwd, encoding: 'utf8', maxBuffer: 1 << 26, env: { ...process.env, TOWER_DATA: '', JET_STATE_ROOT: join(cwd, 'state') } });
  try {
    mkdirSync(join(cwd, '.tower'));
    writeJSON(join(cwd, '.tower', 'tower.json'), empty('Standings CLI'));
    const planned = cli('gauntlet', 'standings');
    assert.equal(planned.status, 0, planned.stderr);
    assert.match(planned.stdout, /^Gauntlet · table state\/gauntlet\/table\.json: nothing measured yet/);
    assert.match(planned.stdout, /\nhello \(batch\)\n {2}Jet +not measured yet\n/);
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
    assert.equal(JSON.parse(json.stdout).entries.find((entry) => entry.entry === 'hello').rows.length, 11);
    const help = cli('help', '--check');
    assert.equal(help.status, 0, help.stderr + help.stdout);
  } finally {
    rmSync(cwd, { recursive: true, force: true });
  }
});
