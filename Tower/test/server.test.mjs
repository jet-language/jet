import { test, after } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { openStore, empty } from '../app/store.mjs';
import { writeJSON } from '../app/paths.mjs';
import { serve } from '../app/server.mjs';

const dir = mkdtempSync(join(tmpdir(), 'tower-srv-'));
writeJSON(join(dir, 'tower.json'), empty('Srv'));
const store = openStore(dir);
const PORT = 7955;
const server = serve(store, PORT, false);
after(() => server.close());

const url = (p) => `http://localhost:${PORT}${p}`;
const post = async (route, body) => {
  const r = await fetch(url('/api/' + route), { method: 'POST', body: JSON.stringify(body) });
  return { status: r.status, json: await r.json() };
};

test('server round-trip: add, state, validation, conflict, next', async () => {
  const add = await post('card/add', { title: 'Via HTTP', by: 'agent-x' });
  assert.equal(add.status, 200);
  assert.equal(add.json.result.num, 1);
  assert.ok(add.json.state.cards.length === 1);

  const state = await (await fetch(url('/api/state'))).json();
  assert.equal(state.meta.project, 'Srv');
  assert.equal(state.cards[0].lane.lane, 'activate');

  const bad = await post('card/update', { id: '#1', phase: 'nope' });
  assert.equal(bad.status, 400);
  assert.equal(bad.json.error, 'E_INVALID');

  const missing = await post('card/update', { id: '#99', title: 'x' });
  assert.equal(missing.status, 404);

  const stale = await post('card/update', { id: '#1', title: 'x', expectRev: 0 });
  assert.equal(stale.status, 409);
  assert.equal(stale.json.error, 'E_CONFLICT');

  await post('card/activate', { id: '#1' });
  const next = await (await fetch(url('/api/next?limit=3'))).json();
  assert.equal(next.length, 1);

  const unknown = await post('nope/nope', {});
  assert.equal(unknown.status, 404);
});

test('server ratify flow advances the card', async () => {
  await post('decision/add', { cardId: '#1', id: 'D-S1', title: 'pick', options: [{ key: 'A', name: 'a' }] });
  let state = await (await fetch(url('/api/state'))).json();
  assert.equal(state.cards[0].lane.lane, 'decide');
  const r = await post('clearance', { decisionId: 'D-S1', outcome: 'A', by: 'owner' });
  assert.equal(r.status, 200);
  state = await (await fetch(url('/api/state'))).json();
  assert.equal(state.cards[0].lane.lane, 'plan');
});
