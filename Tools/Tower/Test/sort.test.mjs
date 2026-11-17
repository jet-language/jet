// `tower card list` and `tower closest` take the board's card sorts with
// --sort; the board UI sorts cards and criteria, keeps the choice per browser,
// and has no Show closed or Count remaining toggle.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import * as db from '../App/store.mjs';
import { writeJSON } from '../App/paths.mjs';

// Run this checkout's CLI; tower.mjs deliberately loads the main checkout's.
const cliModule = new URL('../App/cli.mjs', import.meta.url).href;
const cli = (data, ...args) => spawnSync(process.execPath, ['--input-type=module', '--eval',
  `const { run } = await import(${JSON.stringify(cliModule)}); await run(process.argv.slice(1));`, '--', '--data', data, ...args],
{ encoding: 'utf8', env: { ...process.env, TOWER_DATA: '' } });
const json = (r) => JSON.parse(r.stdout);

function board() {
  const data = mkdtempSync(join(tmpdir(), 'tower-sort-'));
  writeJSON(join(data, 'tower.json'), db.empty('Sort'));
  writeJSON(join(data, 'config.json'), { project: 'Sort' });
  const st = db.openStore(data);
  st.mutate((s, cfg) => {
    db.addCard(s, { title: 'Bravo', priority: 'P2', criteria: ['one', 'two', 'three'], by: 'seed' }, cfg);
    db.addCard(s, { title: 'alpha', priority: 'P0', criteria: ['one'], by: 'seed' }, cfg);
    db.addCard(s, { title: 'Charlie', priority: 'P1', criteria: ['one', 'two'], by: 'seed' }, cfg);
  });
  return data;
}

test('card list --sort orders cards by a board sort key, with :asc or :desc overriding its direction', () => {
  const data = board();
  const nums = (...sort) => {
    const r = cli(data, 'card', 'list', ...sort, '--json');
    assert.equal(r.status, 0, r.stderr);
    return json(r).map(c => c.num);
  };
  assert.deepEqual(nums('--sort', 'priority'), [2, 3, 1]);
  assert.deepEqual(nums('--sort', 'priority:desc'), [1, 3, 2]);
  assert.deepEqual(nums('--sort', 'title'), [2, 1, 3]);
  assert.deepEqual(nums('--sort', 'open'), [2, 3, 1]);
  assert.deepEqual(nums('--sort', 'num:desc'), [3, 2, 1]);

  const bad = cli(data, 'card', 'list', '--sort', 'size', '--json');
  assert.equal(bad.status, 1);
  assert.equal(json(bad).error.code, 'E_USAGE');
  assert.match(json(bad).error.message, /--sort takes one of workflow, workOrder, priority, open, updated, created, num, title, milestone, lane, optionally with :asc or :desc/);
  assert.equal(json(cli(data, 'card', 'list', '--sort', 'num:sideways', '--json')).error.code, 'E_USAGE');
});

test('closest ranks nearest to done by default and takes --sort', () => {
  const data = board();
  const nums = (...args) => {
    const r = cli(data, 'closest', ...args, '--json');
    assert.equal(r.status, 0, r.stderr);
    return json(r).map(c => c.num);
  };
  assert.deepEqual(nums(), [2, 3, 1]);
  assert.deepEqual(nums('--sort', 'open:desc'), [1, 3, 2]);
  assert.deepEqual(nums('--sort', 'title:desc'), [3, 1, 2]);
  assert.deepEqual(nums('--sort', 'priority', '--limit', '1'), [2]);
  assert.equal(json(cli(data, 'closest', '--sort', 'size', '--json')).error.code, 'E_USAGE');
});

test('help lists --sort on card list and closest', () => {
  const help = cli(board(), 'help').stdout;
  assert.match(help, /tower card list \[[^\]]*--sort KEY\[:asc\|desc\]/);
  assert.match(help, /tower closest \[[^\]]*--sort KEY\[:asc\|desc\]/);
});

const source = (file) => readFileSync(new URL(`../App/${file}`, import.meta.url), 'utf8');

test('the board has no Show closed or Count remaining toggle and no closed-card list', () => {
  const ui = ['UI/tower.js', 'UI/board-state.js', 'UI/tower.css', 'UI/index.html'].map(source).join('\n');
  for (const gone of ['Show closed', 'Count remaining', 'radar-closed', 'radar-remaining', 'showClosed', '/api/closed'])
    assert.equal(ui.includes(gone), false, `${gone} is gone from the UI`);
  for (const gone of ['/api/closed', 'projectClosed', 'closed()'])
    assert.equal([source('server.mjs'), source('store.mjs'), source('live.mjs')].join('\n').includes(gone), false, `${gone} is gone from the server`);
});

test('the board, Closest view and card criteria each have a sort control kept per browser', () => {
  const ui = source('UI/tower.js');
  for (const control of ['id="radar-sort"', 'id="closest-sort"', 'id="crit-sort"'])
    assert.ok(ui.includes(control), `${control} renders`);
  for (const name of ['cards', 'closest', 'criteria']) {
    assert.match(ui, new RegExp(`savedSort\\('${name}'`), `${name} sort is restored`);
    assert.match(ui, new RegExp(`saveSort\\('${name}'`), `${name} sort is saved`);
  }
  assert.match(ui, /localStorage\.setItem\(SORT_STORE/);
});
