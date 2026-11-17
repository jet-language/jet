// Epochs are the board's one planning axis: ordered, with measurable exit
// criteria, labelled `E18`, and removable only once nothing points at them.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import * as db from '../app/store.mjs';
import { writeJSON } from '../app/paths.mjs';
import { RETIRED_COLLECTIONS, migrateRetiredPlanning } from '../app/migrate.mjs';

// Run this checkout's CLI; tower.mjs deliberately loads the main checkout's.
const cliModule = new URL('../app/cli.mjs', import.meta.url).href;
const run = (cwd, args, ok = true) => {
  try {
    return { out: execFileSync(process.execPath, ['--input-type=module', '--eval',
      `const { run } = await import(${JSON.stringify(cliModule)}); await run(process.argv.slice(1));`, '--', ...args],
    { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], env: { ...process.env, TOWER_DATA: '' } }), code: 0 };
  } catch (e) {
    if (ok) throw e;
    return { out: (e.stdout || '') + (e.stderr || ''), code: e.status };
  }
};
const code = (expected, pattern) => (e) => e instanceof db.TowerError && e.code === expected && (!pattern || pattern.test(e.message));
const fresh = (state = db.empty('Epochs'), history = null) => {
  const dir = mkdtempSync(join(tmpdir(), 'tower-epochs-'));
  writeJSON(join(dir, 'tower.json'), state);
  if (history) writeJSON(join(dir, 'history.json'), history);
  return { dir, store: db.openStore(dir) };
};

test('an epoch takes an order and exit criteria; bad values are refused and change nothing', () => {
  const { store } = fresh();
  const { result } = store.mutate((s) => db.addEpoch(s, { id: 'e18', name: 'Declarations as types', order: '1', exitCriteria: [' types are values ', 'derives ship'], by: 'owner' }));
  assert.equal(result.order, 1);
  assert.deepEqual(result.exitCriteria, ['types are values', 'derives ship']);
  store.mutate((s) => db.updateEpoch(s, 'e18', { order: 4, exitCriteria: 'one criterion' }));
  assert.deepEqual(store.load().epochs.find(e => e.id === 'e18'), {
    id: 'e18', name: 'Declarations as types', goal: '', status: 'open', order: 4, exitCriteria: ['one criterion'],
  });
  for (const order of [-1, 1.5, 'soon', '', true])
    assert.throws(() => store.mutate((s) => db.updateEpoch(s, 'e18', { order })), code('E_INVALID', /whole number/));
  assert.throws(() => store.mutate((s) => db.updateEpoch(s, 'e18', { exitCriteria: ['ok', '  '] })), code('E_INVALID', /needs text/));
  assert.throws(() => store.mutate((s) => db.updateEpoch(s, 'e18', { status: 'active', order: 'x' })), code('E_INVALID'));
  assert.equal(store.load().epochs.find(e => e.id === 'e18').order, 4, 'a refused update changes nothing');
  store.mutate((s) => db.updateEpoch(s, 'e18', { exitCriteria: [] }));
  assert.deepEqual(store.load().epochs.find(e => e.id === 'e18').exitCriteria, [], 'an empty list clears the criteria');
});

test('epoch remove is refused while a live card, an archived card or a milestone points at it', () => {
  const history = { ...db.emptyHistory(), cards: [{ id: 'old', num: 90, title: 'Archived', phase: 'done', epoch: 'e3', track: 'epoch', criteria: [], log: [] }] };
  const { store } = fresh(db.empty('Epochs'), history);
  store.mutate((s) => { db.addEpoch(s, { id: 'e2', name: 'Two' }); db.addEpoch(s, { id: 'e3', name: 'Three' }); db.addEpoch(s, { id: 'e4', name: 'Four' }); });
  store.mutate((s, cfg) => db.addCard(s, { title: 'Live', epoch: 'e2', by: 'planner' }, cfg));
  store.mutate((s, cfg) => db.addCard(s, { title: 'Also live', epoch: 'e2', phase: 'frozen', by: 'planner' }, cfg));
  store.mutate((s) => db.addMilestone(s, { epochId: 'e2', title: 'Target' }));
  const remove = (id) => store.mutate((s, _cfg, h) => db.removeEpoch(s, h, id, 'owner'));
  assert.throws(() => remove('e2'), code('E_INVALID', /epoch e2 is still referenced by 2 cards, 0 archived cards and 1 milestone; move or delete them first/));
  assert.throws(() => remove('e3'), code('E_INVALID', /0 cards, 1 archived card and 0 milestones/));
  assert.throws(() => remove('e9'), code('E_NOT_FOUND'));
  assert.deepEqual(store.load().epochs.map(e => e.id), ['e1', 'e2', 'e3', 'e4'], 'refusals change nothing');
  assert.deepEqual(remove('e4').result, { removed: 'e4' });
  const s = store.load();
  assert.deepEqual(s.epochs.map(e => e.id), ['e1', 'e2', 'e3']);
  assert.equal(s.events.find(e => e.action === 'epoch.remove').ref, 'e4');
});

test('cli: --order and repeated --exit-criteria, --file JSON lists, ordered E-labelled list, remove refusal', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-epochs-cli-'));
  run(cwd, ['init', '--name', 'Epochs']);
  const added = JSON.parse(run(cwd, ['epoch', 'add', 'e19', '--name', 'Web, end to end', '--order', '2',
    '--exit-criteria', 'serves a page', '--exit-criteria', 'beats the peers', '--by', 'owner', '--json']).out);
  assert.equal(added.order, 2);
  assert.deepEqual(added.exitCriteria, ['serves a page', 'beats the peers']);
  assert.match(run(cwd, ['epoch', 'add', 'e18', '--name', 'Declarations as types', '--by', 'owner']).out, /added epoch E18 Declarations as types/);
  const plan = join(cwd, 'e18.json');
  writeFileSync(plan, JSON.stringify({ order: 1, exitCriteria: ['types are values', 'derives ship'] }));
  run(cwd, ['epoch', 'update', 'e18', '--file', plan, '--by', 'owner']);
  run(cwd, ['epoch', 'update', 'e1', '--order', '3', '--name', 'Leftovers']);

  const listed = JSON.parse(run(cwd, ['epoch', 'list', '--json']).out);
  assert.deepEqual(listed.map(e => e.id), ['e18', 'e19', 'e1']);
  assert.deepEqual(listed[0].exitCriteria, ['types are values', 'derives ship']);
  const text = run(cwd, ['epoch', 'list']).out;
  assert.ok(text.indexOf('E18 Declarations as types') < text.indexOf('E19 Web, end to end'), text);
  assert.ok(text.indexOf('E19 Web, end to end') < text.indexOf('E1 Leftovers'), text);
  assert.match(text, /exit: beats the peers/);
  assert.doesNotMatch(text, /Epoch 18|\be18\b/);

  const bad = run(cwd, ['epoch', 'update', 'e18', '--order', 'first'], false);
  assert.equal(bad.code, 1);
  assert.match(bad.out, /epoch order must be a whole number/);

  run(cwd, ['card', 'add', '--title', 'Typed values', '--epoch', 'e18', '--by', 'planner']);
  const refused = run(cwd, ['epoch', 'remove', 'e18', '--by', 'owner'], false);
  assert.equal(refused.code, 1);
  assert.match(refused.out, /epoch e18 is still referenced by 1 card, 0 archived cards and 0 milestones/);
  assert.match(run(cwd, ['epoch', 'remove', 'e19', '--by', 'owner']).out, /removed epoch e19/);
  assert.deepEqual(JSON.parse(run(cwd, ['epoch', 'list', '--json']).out).map(e => e.id), ['e18', 'e1']);

  // `next --parallel` names each card's epoch by its label.
  assert.match(run(cwd, ['next', '--parallel']).out, /#1 +E18 /);
});

test('the store drops the retired planning collections and every card goal link, live and archived', () => {
  const state = db.empty('Retired');
  for (const key of RETIRED_COLLECTIONS) state[key] = [{ id: `${key}-1`, text: 'retired', status: 'open' }];
  state.cards = [{ id: 'c1', num: 1, title: 'Linked', phase: 'planning', track: 'epoch', epoch: 'e1', criteria: [], log: [], goals: ['G1'] }];
  state.meta.nextNum = 2;
  const history = { ...db.emptyHistory(), cards: [{ id: 'old', num: 90, title: 'Archived', phase: 'done', epoch: 'e1', track: 'epoch', criteria: [], log: [], goals: ['G1'] }] };
  const { dir, store } = fresh(state, history);
  const kept = (obj) => RETIRED_COLLECTIONS.filter(key => key in obj);

  const loaded = store.load();
  assert.deepEqual(kept(loaded), []);
  assert.deepEqual(kept(db.project(loaded)), []);
  assert.equal('goals' in loaded.cards[0], false);
  assert.equal('goals' in store.loadHistory().cards[0], false);

  store.mutate((s) => db.updateEpoch(s, 'e1', { order: 1 }));
  const live = JSON.parse(readFileSync(join(dir, 'tower.json'), 'utf8'));
  assert.deepEqual(kept(live), [], 'the next write persists the board without them');
  assert.equal('goals' in live.cards[0], false);
  assert.equal(live.cards[0].title, 'Linked', 'the card is otherwise unchanged');
  assert.equal(readFileSync(join(dir, 'history.json'), 'utf8').includes('"goals"'), false, 'the archive is rewritten too');

  const board = { cards: [{ id: 'c1', goals: ['G1'] }, { id: 'c2' }] };
  for (const key of RETIRED_COLLECTIONS) board[key] = [];
  assert.equal(migrateRetiredPlanning(board), true);
  assert.deepEqual(board, { cards: [{ id: 'c1' }, { id: 'c2' }] });
  assert.equal(migrateRetiredPlanning(board), false, 'the migration is idempotent');
});
