import { test, after } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { join } from 'node:path';
import { openStore, empty, emptyHistory } from '../app/store.mjs';
import { configFile, historyFile, writeJSON } from '../app/paths.mjs';
import { serve } from '../app/server.mjs';

const dir = mkdtempSync(join(process.cwd(), '.tower-perf-'));
const initial = empty('Perf');
initial.meta.completionCursor = '2020-01-01T00:00:00.000Z';
writeJSON(join(dir, 'tower.json'), initial);
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
  const response = await fetch(url(`/api/${route}`), {
    method: 'POST', headers: { 'content-type': 'application/json', 'x-tower-client': 'cli' }, body: JSON.stringify(payload),
  });
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
  assert.equal(Object.hasOwn(state, 'closed'), false);
  assert.equal(state.notices.cards.some(c => c.num === added.json.result.num), true);

  const stateCached = await fetch(url('/api/state'), {
    headers: { 'accept-encoding': 'gzip', 'if-none-match': stateTag },
  });
  assert.equal(stateCached.status, 304);

  const closedResponse = await fetch(url('/api/closed'), { headers: { 'accept-encoding': 'gzip' } });
  assert.equal(closedResponse.headers.get('content-encoding'), 'gzip');
  const closed = await closedResponse.json();
  assert.equal(closedResponse.status, 200, JSON.stringify(closed));
  const closedCard = closed.cards.find(c => c.num === added.json.result.num);
  assert.equal(closedCard.phase, 'done');
  assert.equal(closedCard.title, 'Closed through API');
  assert.equal(Object.hasOwn(closedCard, 'body'), false, 'closed list rows are summaries; bodies load per card');
  assert.equal(Object.hasOwn(closedCard, 'log'), false);

  const detail = await (await fetch(url(`/api/card?id=${encodeURIComponent(ref)}`))).json();
  assert.equal(detail.card.phase, 'done');
  assert.equal(detail.card.title, 'Closed through API');
  assert.equal(typeof detail.card.body, 'string');
  assert.equal(detail.card.criteria[0].status, 'met');
  // The active board must not touch the closed-card store. A broken archive
  // may break /api/closed, but it cannot make the default board unavailable.
  writeFileSync(historyFile(dir), '{broken');
  const activeOnly = await fetch(url('/api/state'));
  assert.equal(activeOnly.status, 200);
  assert.equal(Object.hasOwn(await activeOnly.json(), 'closed'), false);
});

test('each CLI-style write process reuses an identical history backup instead of copying the archive again', () => {
  const data = mkdtempSync(join(process.cwd(), '.tower-perf-backup-'));
  try {
    writeJSON(join(data, 'tower.json'), empty('Backups'));
    writeJSON(configFile(data), { project: 'Backups' });
    writeJSON(historyFile(data), emptyHistory());
    const storeUrl = new URL('../app/store.mjs', import.meta.url).href;
    const write = (title) => {
      const run = spawnSync(process.execPath, ['--input-type=module', '-e',
        `import { openStore, addCard } from ${JSON.stringify(storeUrl)};
         openStore(${JSON.stringify(data)}).mutate((s, cfg) => addCard(s, { title: ${JSON.stringify(title)}, by: 'Pip' }, cfg));`,
      ], { encoding: 'utf8' });
      assert.equal(run.status, 0, run.stderr);
    };
    const historyBackups = () => readdirSync(join(data, 'backups')).filter(name => name.startsWith('history-'));
    write('first');
    write('second');
    write('third');
    assert.equal(historyBackups().length, 1, 'unchanged archive: one history backup serves every write');
    assert.equal(readdirSync(join(data, 'backups')).filter(name => name.startsWith('tower-')).length, 3, 'undo keeps one live backup per write');
    assert.equal(readFileSync(join(data, 'backups', historyBackups()[0]), 'utf8'), readFileSync(historyFile(data), 'utf8'));
    writeJSON(historyFile(data), { ...emptyHistory(), events: [{ type: 'note', at: '2026-10-31T00:00:00.000Z' }] });
    write('fourth');
    assert.equal(historyBackups().length, 2, 'a changed archive gets a fresh backup');
  } finally {
    rmSync(data, { recursive: true, force: true });
  }
});

