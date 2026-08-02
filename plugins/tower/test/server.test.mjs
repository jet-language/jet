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
  assert.equal(Object.hasOwn(secretShape, 'push'), false);

  const state = await (await fetch(url('/api/state'))).json();
  assert.equal(state.meta.project, 'Srv');
  assert.equal(state.cards[0].lane.lane, 'plan', 'no greenlight gate — a fresh card lands agent-ready');

  const bad = await post('card/update', { id: '#1', phase: 'nope' });
  assert.equal(bad.status, 400);
  assert.equal(bad.json.error, 'E_INVALID');

  const missing = await post('card/update', { id: '#99', title: 'x' });
  assert.equal(missing.status, 404);

  const stale = await post('card/update', { id: '#1', title: 'x', expectRev: 0 });
  assert.equal(stale.status, 409);
  assert.equal(stale.json.error, 'E_CONFLICT');

  const next = await (await fetch(url('/api/next?limit=3'))).json();
  assert.equal(next.length, 1);

  const unknown = await post('nope/nope', {});
  assert.equal(unknown.status, 404);
});

test('message API adds, lists, and closes independently of done-card clearing', async () => {
  const add = await post('message/add', {
    cardId: '#1',
    text: 'Read the migration note.',
    by: 'agent-x',
  });
  assert.equal(add.status, 200);
  assert.equal(add.json.result.kind, 'message');

  const listed = await (await fetch(url('/api/messages'))).json();
  assert.deepEqual(listed.map(message => message.id), [add.json.result.id]);

  const rejectedClear = await post('done/clear', { by: 'agent-x' });
  assert.equal(rejectedClear.status, 403);
  assert.equal(rejectedClear.json.error, 'E_OWNER_ONLY');

  const cleared = await post('done/clear', { by: 'owner' });
  assert.equal(cleared.status, 200);
  assert.equal(cleared.json.state.events[0].action, 'done.clear');
  assert.equal(cleared.json.state.events[0].by, 'owner');
  const afterClear = await (await fetch(url('/api/messages'))).json();
  assert.equal(afterClear.length, 1, 'clearing completed cards must not clear messages');

  const rejected = await post('message/done', { id: add.json.result.id, by: 'agent-x' });
  assert.equal(rejected.status, 403);
  assert.equal(rejected.json.error, 'E_OWNER_ONLY');

  const done = await post('message/done', { id: add.json.result.id, by: 'owner' });
  assert.equal(done.status, 200);
  assert.equal(done.json.result.status, 'done');
  assert.deepEqual(await (await fetch(url('/api/messages'))).json(), []);
  const all = await (await fetch(url('/api/messages?status='))).json();
  assert.equal(all[0].doneBy, 'owner');
});

// #522 — stale-process trap: /api/version reports what this process loaded
// at boot vs. a fresh read of the source on disk right now.
test('GET /api/version reports start/current/stale', async () => {
  const v = await (await fetch(url('/api/version'))).json();
  assert.equal(typeof v.start, 'string');
  assert.equal(typeof v.current, 'string');
  assert.equal(v.stale, false, 'source on disk has not changed since this process booted');
  assert.equal(v.current, v.start);
});

// #522 — index.html always carries the CURRENT on-disk version (it's
// re-read fresh every request), independent of what the process loaded.
test('served index.html stamps a live tower-version meta tag', async () => {
  const html = await (await fetch(url('/'))).text();
  const m = /<meta name="tower-version" content="([^"]+)">/.exec(html);
  assert.ok(m, 'index.html carries the tower-version meta tag');
  assert.notEqual(m[1], '__TOWER_VERSION__', 'placeholder was replaced');
  const v = await (await fetch(url('/api/version'))).json();
  assert.equal(m[1], v.current, 'stamped marker matches a fresh on-disk read');
});

test('server ratify flow advances the card', async () => {
  await post('decision/add', { cardId: '#1', id: 'D-S1', title: 'pick',
    ballotMode: 'full', reviewPasses: { base: 'The base pass completed the ballot.', boilOcean: 'The boil-the-ocean pass tested the broad solution space.', hybrid: 'The hybrid pass combined compatible strengths.', cooperative: 'The cooperative pass strengthened each option.', adversarial: 'The adversarial pass attacked the recommendation.' },
    gist: 'g', lesson: 'teach from zero', story: 's', inWild: 'w', rec: 'A',
    recommendation: { why: 'A wins here.', whyNot: [{ key: 'B', reason: 'B loses the needed behavior.' }], tradeoff: 'A adds one visible step.' },
    hybrid: { result: 'A', synthesis: 'A combines the useful parts.', harvest: [{ key: 'A', aspect: 'A is explicit.', use: 'Keep it.' }, { key: 'B', aspect: 'B is brief.', use: 'Borrow its short names.' }] },
    options: [{ key: 'A', name: 'a', detail: 'A is explicit.', code: 'a()' }, { key: 'B', name: 'b', detail: 'B is brief.', code: 'b()' }] });
  let state = await (await fetch(url('/api/state'))).json();
  assert.equal(state.cards[0].lane.lane, 'decide');
  assert.equal(state.decisions[0].ballotMode, 'full');
  assert.equal(state.decisions[0].reviewPasses.cooperative, 'The cooperative pass strengthened each option.');
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
