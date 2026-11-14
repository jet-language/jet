// #6834: the standings projection the Gauntlet page and `tower gauntlet
// standings` share, for a peers-only run (Jet pending) and a run with Jet
// merged, the results-file lookup, and the CLI table.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { empty, TowerError } from '../app/store.mjs';
import { writeJSON } from '../app/paths.mjs';
import { readGauntletStandings, sharedResultsDir, standingsLocations } from '../app/gauntlet-standings.mjs';
import { gauntletStandingsText, projectGauntletStandings, renderGauntletStandings } from '../app/ui/gauntlet.js';

const FIXTURE = fileURLToPath(new URL('./fixtures/gauntlet-results.json', import.meta.url));
const PEERS_FIXTURE = fileURLToPath(new URL('./fixtures/gauntlet-results-peers.json', import.meta.url));
const report = (file = FIXTURE) => JSON.parse(readFileSync(file, 'utf8'));
const view = (file = FIXTURE) => projectGauntletStandings({ file: 'results.json', scope: 'partial_entry', standings: report(file).standings });
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
  assert.deepEqual(projected.peers.map((peer) => peer.peer), ['rust', 'c', 'zig', 'go', 'python', 'bun', 'node', 'vite', 'asm']);
  assert.deepEqual(projected.metrics.map((metric) => metric.id), ['startup', 'wall', 'cycles', 'peak_rss', 'size']);
  for (const item of projected.entries[0].rows) assert.equal(item.cells.length, 5);
  assert.deepEqual(projected.summary, { floor: { pass: 29, fail: 1, other: 10 }, ceiling: { meet: 0, miss: 5, other: 0 } });
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
  assert.match(text, /^Gauntlet standings · results results\.json · partial entry · generated /);
  assert.match(text, /floor 29 pass, 1 fail, 10 other · ceiling 0 meet, 5 miss, 0 other/);
  assert.match(text, /\nsize basis \(each size value names its basis\):\n {2}static stripped: Jet, Rust, C, Zig, Go and asm/);
  assert.match(text, /\nhello \(batch\)\n {2}peer +startup +run wall +run cycles +peak RSS +size\n {2}Jet +331 µs +417 µs +57\.8 k cycles +292 kB +95\.6 kB\n/);
  assert.match(text, /\n {2}C +683 µs 0\.48 pass +851 µs 0\.49 pass +617 k cycles 0\.09 pass +1\.95 MB 0\.15 pass +13\.4 kB 7\.16 fail\n/);
  assert.match(text, /\n {2}size basis: Jet static stripped · Rust stripped \+ shared libraries · C static stripped · Zig static stripped · Python script\+runtime · Bun script\+runtime · Node script\+runtime · asm ceiling static stripped(\n|$)/);
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

test('the results lookup prefers the newest full run, else the newest partial run, across every location', () => {
  const root = mkdtempSync(join(tmpdir(), 'tower-standings-'));
  try {
    const dir = join(root, 'results');
    const shared = join(root, 'shared');
    assert.throws(() => readGauntletStandings(root, [dir, shared]), (error) => error instanceof TowerError && error.code === 'E_NOT_FOUND' && / do not exist$/.test(error.message));
    mkdirSync(dir);
    assert.throws(() => readGauntletStandings(root, [dir, shared]), /no results file in .* or .* has a standings section/);
    const partial = report();
    writeJSON(join(dir, '2026-11-14-hello.json'), partial);
    writeJSON(join(dir, '2026-11-12-hello.json'), partial);
    writeJSON(join(dir, '2026-11-15-axis-live_reload.json'), { contract: 'gauntlet-report-v1' });
    assert.equal(readGauntletStandings(root, [dir, shared]).file, join('results', '2026-11-14-hello.json'));
    mkdirSync(shared);
    writeJSON(join(shared, '2026-11-15-languages-rust,c.json'), report(PEERS_FIXTURE));
    assert.equal(readGauntletStandings(root, [dir, shared]).file, join('shared', '2026-11-15-languages-rust,c.json'));
    writeJSON(join(dir, '2026-11-13.json'), { ...partial, options: { scope: 'full_matrix' } });
    const full = readGauntletStandings(root, [dir, shared]);
    assert.equal(full.file, join('results', '2026-11-13.json'));
    assert.equal(full.scope, 'full_matrix');
    writeJSON(join(shared, '2026-11-15-merged-languages-asm.json'), { ...report(PEERS_FIXTURE), options: { scope: 'full_matrix' } });
    assert.equal(readGauntletStandings(root, [dir, shared]).file, join('shared', '2026-11-15-merged-languages-asm.json'));
    assert.equal(readGauntletStandings(root, [join(dir, '2026-11-12-hello.json')]).scope, 'partial_entry');
    assert.throws(() => readGauntletStandings(root, [join(dir, '2026-11-15-axis-live_reload.json')]), (error) => error.code === 'E_INVALID');
    writeFileSync(join(dir, '2026-11-16.json'), '{');
    assert.throws(() => readGauntletStandings(root, [dir, shared]), (error) => error.code === 'E_INVALID');
    const state = process.env.JET_STATE_ROOT;
    process.env.JET_STATE_ROOT = join(root, 'state');
    try {
      assert.equal(sharedResultsDir(), join(root, 'state', 'gauntlet', 'results'));
      assert.deepEqual(standingsLocations(root, 'Tools/gauntlet/results'), [join(root, 'Tools', 'gauntlet', 'results'), sharedResultsDir()]);
      assert.deepEqual(standingsLocations(root, dir), [dir, sharedResultsDir()]);
    } finally {
      if (state === undefined) delete process.env.JET_STATE_ROOT;
      else process.env.JET_STATE_ROOT = state;
    }
    assert.deepEqual(standingsLocations(root, 'Tools/gauntlet/results', dir), [dir]);
    assert.throws(() => standingsLocations(root, ''), (error) => error.code === 'E_USAGE');
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('`tower gauntlet standings` prints the table from --results or the shared results directory', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-standings-cli-'));
  const cliModule = new URL('../app/cli.mjs', import.meta.url).href;
  const cli = (...args) => spawnSync(process.execPath, ['--input-type=module', '--eval',
    `const { run } = await import(${JSON.stringify(cliModule)}); await run(process.argv.slice(1));`,
    '--', ...args], { cwd, encoding: 'utf8', env: { ...process.env, TOWER_DATA: '', JET_STATE_ROOT: join(cwd, 'state') } });
  try {
    mkdirSync(join(cwd, '.tower'));
    writeJSON(join(cwd, '.tower', 'tower.json'), empty('Standings CLI'));
    const shown = cli('gauntlet', 'standings', '--results', FIXTURE);
    assert.equal(shown.status, 0, shown.stderr);
    assert.equal(shown.stdout.trimEnd(), gauntletStandingsText(projectGauntletStandings({ file: FIXTURE, scope: 'partial_entry', standings: report().standings })));
    const json = cli('gauntlet', 'standings', '--results', FIXTURE, '--json');
    assert.equal(json.status, 0, json.stderr);
    assert.equal(JSON.parse(json.stdout).entries[0].rows.length, 9);
    const missing = cli('gauntlet', 'standings');
    assert.notEqual(missing.status, 0);
    assert.match(missing.stderr + missing.stdout, /Gauntlet standings are unavailable: .*Tools\/gauntlet\/results and .*state\/gauntlet\/results do not exist/);
    const name = '2026-05-14-languages-rust,c,zig,go,python,node,bun,vite,lua,luajit.json';
    mkdirSync(join(cwd, 'state', 'gauntlet', 'results'), { recursive: true });
    writeFileSync(join(cwd, 'state', 'gauntlet', 'results', name), readFileSync(PEERS_FIXTURE));
    const peers = cli('gauntlet', 'standings');
    assert.equal(peers.status, 0, peers.stderr);
    const file = join('state', 'gauntlet', 'results', name);
    assert.equal(peers.stdout.trimEnd(), gauntletStandingsText(projectGauntletStandings({ file, scope: 'partial_entry', standings: report(PEERS_FIXTURE).standings })));
    assert.match(peers.stdout, /\nhello \(batch\) · Jet pending\n/);
    const help = cli('help', '--check');
    assert.equal(help.status, 0, help.stderr + help.stdout);
  } finally {
    rmSync(cwd, { recursive: true, force: true });
  }
});
