// #6834: the standings projection the Gauntlet page and `tower gauntlet
// standings` share, the results-file lookup, and the CLI table.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { empty, TowerError } from '../app/store.mjs';
import { writeJSON } from '../app/paths.mjs';
import { readGauntletStandings, standingsLocation } from '../app/gauntlet-standings.mjs';
import { gauntletStandingsText, projectGauntletStandings, renderGauntletStandings } from '../app/ui/gauntlet.js';

const FIXTURE = fileURLToPath(new URL('./fixtures/gauntlet-results.json', import.meta.url));
const report = () => JSON.parse(readFileSync(FIXTURE, 'utf8'));
const view = () => projectGauntletStandings({ file: 'results.json', scope: 'partial_entry', standings: report().standings });
const row = (projected, peer) => projected.entries[0].rows.find((item) => item.peer === peer);
const cell = (projected, peer, metric) => row(projected, peer).cells.find((item) => item.metric === metric);

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

test('the text table and the page render the same projection', () => {
  const projected = view();
  const text = gauntletStandingsText(projected);
  assert.match(text, /^Gauntlet standings · results results\.json · partial entry · generated /);
  assert.match(text, /floor 29 pass, 1 fail, 10 other · ceiling 0 meet, 5 miss, 0 other/);
  assert.match(text, /\nsize basis \(each size cell names the Jet and the peer basis\):\n {2}static stripped: Jet, Rust, C, Zig, Go and asm/);
  assert.match(text, /\n {2}stripped \+ shared libraries: a compiled program with no static build/);
  assert.match(text, /\n {2}C +0\.48 pass +0\.49 pass +0\.09 pass +0\.15 pass +7\.16 fail\n/);
  assert.match(text, /\n {2}size basis: Jet static stripped · Rust stripped \+ shared libraries · C static stripped · Zig static stripped · Python script\+runtime · Bun script\+runtime · Node script\+runtime · asm ceiling static stripped(\n|$)/);
  assert.match(text, /\n {2}Go +uncovered: no go program: go\/main\.go is missing\n/);
  assert.match(text, /\n {2}Vite +not applicable: Vite bundles web front ends/);
  assert.match(text, /\n {2}asm ceiling +1\.12 miss \+12% +1\.12 miss \+12% +6\.01 miss \+501% +4\.06 miss \+306% +65\.76 miss \+6476%/);
  const html = renderGauntletStandings(projected);
  assert.equal(html.match(/<tr class="standings__row/g).length, 9);
  assert.match(html, /<tr class="standings__row standings__row--ceiling">/);
  assert.match(html, /standings__cell standings__cell--fail" title="hello vs C, size: Jet 95\.6 kB \/ 13\.4 kB \(Jet static stripped, peer static stripped\)"><b>7\.16 fail<\/b><span><i>95\.6 kB<\/i><i>13\.4 kB<\/i><\/span><em>static stripped<\/em><\/td>/);
  assert.match(html, /<p class="gauntlet__policy standings__basis"><strong>size basis \(each size cell names the Jet and the peer basis\):<\/strong><span>static stripped: /);
  assert.match(html, /colspan="5" class="standings__cell standings__cell--open"><b>uncovered<\/b>/);
  assert.match(html, /colspan="5" class="standings__cell standings__cell--na"><b>not applicable<\/b>/);
});

test('the results lookup prefers the newest full run, else the newest partial run', () => {
  const root = mkdtempSync(join(tmpdir(), 'tower-standings-'));
  try {
    const dir = join(root, 'results');
    assert.throws(() => readGauntletStandings(root, dir), (error) => error instanceof TowerError && error.code === 'E_NOT_FOUND');
    mkdirSync(dir);
    assert.throws(() => readGauntletStandings(root, dir), /no results file in .* has a standings section/);
    const partial = report();
    writeJSON(join(dir, '2026-11-14-hello.json'), partial);
    writeJSON(join(dir, '2026-11-12-hello.json'), partial);
    writeJSON(join(dir, '2026-11-15-axis-live_reload.json'), { contract: 'gauntlet-report-v1' });
    assert.equal(readGauntletStandings(root, dir).file, join('results', '2026-11-14-hello.json'));
    writeJSON(join(dir, '2026-11-13.json'), { ...partial, options: { scope: 'full_matrix' } });
    const full = readGauntletStandings(root, dir);
    assert.equal(full.file, join('results', '2026-11-13.json'));
    assert.equal(full.scope, 'full_matrix');
    assert.equal(readGauntletStandings(root, join(dir, '2026-11-12-hello.json')).scope, 'partial_entry');
    assert.throws(() => readGauntletStandings(root, join(dir, '2026-11-15-axis-live_reload.json')), (error) => error.code === 'E_INVALID');
    writeFileSync(join(dir, '2026-11-16.json'), '{');
    assert.throws(() => readGauntletStandings(root, dir), (error) => error.code === 'E_INVALID');
    assert.equal(standingsLocation(root, 'Tools/gauntlet/results'), join(root, 'Tools', 'gauntlet', 'results'));
    assert.equal(standingsLocation(root, 'Tools/gauntlet/results', dir), dir);
    assert.throws(() => standingsLocation(root, ''), (error) => error.code === 'E_USAGE');
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('`tower gauntlet standings` prints the table from --results', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-standings-cli-'));
  const cliModule = new URL('../app/cli.mjs', import.meta.url).href;
  const cli = (...args) => spawnSync(process.execPath, ['--input-type=module', '--eval',
    `const { run } = await import(${JSON.stringify(cliModule)}); await run(process.argv.slice(1));`,
    '--', ...args], { cwd, encoding: 'utf8', env: { ...process.env, TOWER_DATA: '' } });
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
    assert.match(missing.stderr + missing.stdout, /Gauntlet standings are unavailable: .*Tools\/gauntlet\/results does not exist/);
    const help = cli('help', '--check');
    assert.equal(help.status, 0, help.stderr + help.stdout);
  } finally {
    rmSync(cwd, { recursive: true, force: true });
  }
});
