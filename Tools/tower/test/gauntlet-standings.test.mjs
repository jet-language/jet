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
  assert.equal(cell(projected, 'c', 'size').text, '6.92 fail');
  assert.equal(cell(projected, 'rust', 'startup').text, '0.37 pass');
  assert.equal(cell(projected, 'asm', 'startup').text, '1.14 miss +14%');
  assert.equal(row(projected, 'asm').role, 'ceiling');
  assert.match(cell(projected, 'python', 'size').detail, /peer script\+runtime/);
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
});

test('the text table and the page render the same projection', () => {
  const projected = view();
  const text = gauntletStandingsText(projected);
  assert.match(text, /^Gauntlet standings · results results\.json · partial entry · generated /);
  assert.match(text, /floor 29 pass, 1 fail, 10 other · ceiling 0 meet, 5 miss, 0 other/);
  assert.match(text, /\n {2}C +0\.59 pass +0\.63 pass +0\.15 pass +0\.15 pass +6\.92 fail\n/);
  assert.match(text, /\n {2}Go +uncovered: no go program: go\/main\.go is missing\n/);
  assert.match(text, /\n {2}Vite +not applicable: Vite bundles web front ends/);
  assert.match(text, /\n {2}asm ceiling +1\.14 miss \+14% +1\.08 miss \+8% +5\.31 miss \+431% +4\.29 miss \+329% +75\.69 miss \+7469%/);
  const html = renderGauntletStandings(projected);
  assert.equal(html.match(/<tr class="standings__row/g).length, 9);
  assert.match(html, /<tr class="standings__row standings__row--ceiling">/);
  assert.match(html, /standings__cell standings__cell--fail" title="hello vs C, size: Jet 110 kB \/ 15\.9 kB \(Jet binary, peer binary\)"><b>6\.92 fail<\/b>/);
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
