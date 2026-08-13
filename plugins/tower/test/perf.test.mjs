import { test, after } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { openStore, empty } from '../app/store.mjs';
import { configFile, writeJSON } from '../app/paths.mjs';
import { serve } from '../app/server.mjs';

const dir = mkdtempSync(join(process.cwd(), '.tower-perf-'));
writeJSON(join(dir, 'tower.json'), empty('Perf'));
writeJSON(configFile(dir), { project: 'Perf' });
const store = openStore(dir);
const PORT = 7977;
const server = serve(store, PORT, false);
after(async () => {
  await new Promise(resolve => server.close(resolve));
  rmSync(dir, { recursive: true, force: true });
});

const url = (path) => `http://localhost:${PORT}${path}`;
const post = async (route, payload) => {
  const response = await fetch(url(`/api/${route}`), { method: 'POST', body: JSON.stringify(payload) });
  return { status: response.status, json: await response.json() };
};

test('board state is slim, closed content is lazy, and HTTP responses cache safely', async () => {
  const page = await fetch(url('/'), { headers: { 'accept-encoding': 'gzip' } });
  assert.equal(page.headers.get('content-encoding'), 'gzip');
  const pageTag = page.headers.get('etag');
  assert.ok(pageTag);
  const pageCached = await fetch(url('/'), { headers: { 'accept-encoding': 'gzip', 'if-none-match': pageTag } });
  assert.equal(pageCached.status, 304);
  const script = await fetch(url('/tower.js'), { headers: { 'accept-encoding': 'gzip' } });
  assert.equal(script.headers.get('content-encoding'), 'gzip');

  const added = await post('card/add', { title: 'Closed through API', by: 'agent-x' });
  assert.equal(added.status, 200);
  const ref = `#${added.json.result.num}`;
  assert.equal((await post('card/criteria-add', { id: ref, text: 'closed content is readable', by: 'builder' })).status, 200);
  assert.equal((await post('card/criteria-meet', { id: ref, n: 1, evidence: 'test', by: 'builder' })).status, 200);
  assert.equal((await post('card/update', { id: ref, phase: 'done', by: 'builder' })).status, 200);

  const stateResponse = await fetch(url('/api/state'), { headers: { 'accept-encoding': 'gzip' } });
  assert.equal(stateResponse.headers.get('content-encoding'), 'gzip');
  const stateTag = stateResponse.headers.get('etag');
  assert.ok(stateTag);
  const state = await stateResponse.json();
  assert.ok(Buffer.byteLength(JSON.stringify(state)) < 1_000_000);
  assert.equal(state.cards.some(c => c.num === added.json.result.num), false);

  const stateCached = await fetch(url('/api/state'), {
    headers: { 'accept-encoding': 'gzip', 'if-none-match': stateTag },
  });
  assert.equal(stateCached.status, 304);

  const closedResponse = await fetch(url('/api/closed'), { headers: { 'accept-encoding': 'gzip' } });
  assert.equal(closedResponse.headers.get('content-encoding'), 'gzip');
  const closed = await closedResponse.json();
  const closedCard = closed.cards.find(c => c.num === added.json.result.num);
  assert.equal(typeof closedCard.body, 'string');
  assert.equal(closedCard.phase, 'done');

  const detail = await (await fetch(url(`/api/card?id=${encodeURIComponent(ref)}`))).json();
  assert.equal(detail.card.phase, 'done');
  assert.equal(detail.card.title, 'Closed through API');
});
