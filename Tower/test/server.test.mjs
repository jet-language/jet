import { test, after } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { openStore, empty } from '../app/store.mjs';
import { configFile, readJSON, secretsFile, writeJSON } from '../app/paths.mjs';
import { serve } from '../app/server.mjs';

const dir = mkdtempSync(join(tmpdir(), 'tower-srv-'));
writeJSON(join(dir, 'tower.json'), empty('Srv'));
writeJSON(configFile(dir), { project: 'Srv' });
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
  assert.equal(Object.hasOwn(add.json.state.config, 'auth'), false);
  assert.equal(Object.hasOwn(add.json.state.config, 'push'), false);
  assert.equal(Object.hasOwn(readJSON(configFile(dir), {}), 'push'), false);
  const secretShape = readJSON(secretsFile(dir), {});
  assert.equal(typeof secretShape.push?.privateJwk, 'object');
  assert.equal(Array.isArray(secretShape.push?.subscriptions), true);

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

  await post('card/activate', { id: '#1', by: 'owner' });
  const next = await (await fetch(url('/api/next?limit=3'))).json();
  assert.equal(next.length, 1);

  const unknown = await post('nope/nope', {});
  assert.equal(unknown.status, 404);
});

test('server ratify flow advances the card', async () => {
  await post('decision/add', { cardId: '#1', id: 'D-S1', title: 'pick',
    gist: 'g', story: 's', inWild: 'w', rec: 'A',
    options: [{ key: 'A', name: 'a', code: 'a()' }, { key: 'B', name: 'b', code: 'b()' }] });
  let state = await (await fetch(url('/api/state'))).json();
  assert.equal(state.cards[0].lane.lane, 'decide');
  const r = await post('clearance', { decisionId: 'D-S1', outcome: 'A', by: 'owner' });
  assert.equal(r.status, 200);
  state = await (await fetch(url('/api/state'))).json();
  assert.equal(state.cards[0].lane.lane, 'plan');
});

// #462 — GET /api/brief
test('GET /api/brief returns the packet and only claims when agent+claim=1', async () => {
  const readOnly = await (await fetch(url('/api/brief?card=1'))).json();
  assert.deepEqual(new Set(Object.keys(readOnly)), new Set(['card', 'blockers', 'criteria', 'decisions', 'questions', 'refs', 'log', 'rules']));
  assert.equal(readOnly.card.assignee, null, 'no agent+claim → never assigns');
  assert.ok(readOnly.decisions.find(d => d.id === 'D-S1' && d.status === 'ratified' && d.outcome === 'A'));

  const noClaimYet = await (await fetch(url('/api/brief?card=1&agent=srv-agent'))).json();
  assert.equal(noClaimYet.card.assignee, null, 'agent without claim=1 does not claim');

  const claimed = await (await fetch(url('/api/brief?card=1&agent=srv-agent&claim=1'))).json();
  assert.equal(claimed.card.assignee, 'srv-agent');

  const missing = await fetch(url('/api/brief?card=999'));
  assert.equal(missing.status, 404);
});
