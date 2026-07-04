import { test, after } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import * as db from '../app/store.mjs';
import { openStore, empty } from '../app/store.mjs';
import { writeJSON } from '../app/paths.mjs';
import { serve } from '../app/server.mjs';

const dir = mkdtempSync(join(tmpdir(), 'tower-wave-'));
writeJSON(join(dir, 'tower.json'), empty('Wave'));
// fast batch window + a declared agent so notifications have a target
writeJSON(join(dir, 'config.json'), { project: 'Wave', notifyBatchSeconds: 0.15, agents: [{ name: 'a1', kind: 'claude' }] });
const store = openStore(dir);
const PORT = 7957;
const server = serve(store, PORT, false);
after(() => server.close());

const url = (p) => `http://localhost:${PORT}${p}`;
const post = async (p, b, raw = false) => {
  const r = await fetch(url(p), { method: 'POST', body: raw ? b : JSON.stringify(b) });
  return { status: r.status, json: await r.json().catch(() => null) };
};

test('auth provisioning: token + vapid persisted to config', () => {
  assert.ok(store.config.auth?.token?.length > 10);
  assert.ok(store.config.push?.publicKey?.length > 40);
});

test('undo: revert last write, conflict-guarded', async () => {
  await post('/api/card/add', { title: 'keep me' });
  const before = await (await fetch(url('/api/state'))).json();
  await post('/api/card/add', { title: 'oops' });
  const mid = await (await fetch(url('/api/state'))).json();
  assert.equal(mid.cards.length, 2);

  // stale expectRev → refused
  const bad = await post('/api/undo', { expectRev: before.meta.rev });
  assert.equal(bad.status, 409);

  const ok = await post('/api/undo', { expectRev: mid.meta.rev });
  assert.equal(ok.status, 200);
  const after2 = await (await fetch(url('/api/state'))).json();
  assert.equal(after2.cards.length, 1);
  assert.equal(after2.cards[0].title, 'keep me');
  assert.equal(after2.meta.rev, mid.meta.rev + 1);
});

test('file upload + download round-trip', async () => {
  const r = await fetch(url('/api/file?name=shot.png&type=image/png'), { method: 'POST', body: Buffer.from([1, 2, 3, 4]) });
  const j = await r.json();
  assert.ok(j.ok && j.file.id);
  const got = await fetch(url('/files/' + j.file.id));
  assert.equal(got.headers.get('content-type'), 'image/png');
  assert.equal((await got.arrayBuffer()).byteLength, 4);
});

test('agent status shows in roster', async () => {
  await post('/api/agent/status', { name: 'a1', kind: 'claude', text: 'building #3 — tests green' });
  const roster = await (await fetch(url('/api/agents'))).json();
  const a1 = roster.find(a => a.name === 'a1');
  assert.ok(a1.online);
  assert.equal(a1.statusText, 'building #3 — tests green');
});

test('batched notify: several ratifications → ONE tower message per agent', async () => {
  const { json: cardR } = await post('/api/card/add', { title: 'ballot host' });
  const cid = cardR.result.id;
  for (const n of [1, 2, 3]) await post('/api/decision/add', { cardId: cid, id: 'D-W' + n, title: 'w' + n, options: [{ key: 'A', name: 'a' }] });
  await post('/api/clearance', { decisionId: 'D-W1', outcome: 'A', by: 'owner' });
  await post('/api/clearance/batch', { by: 'owner', decisions: [{ decisionId: 'D-W2', outcome: 'A' }, { decisionId: 'D-W3', outcome: 'A' }] });
  await new Promise(r => setTimeout(r, 500));   // > batch window
  const s = store.load();
  const towerMsgs = s.messages.filter(m => m.from === 'tower' && m.to === 'a1');
  assert.equal(towerMsgs.length, 1, 'exactly one batched notification');
  assert.match(towerMsgs[0].text, /3 decisions ratified/);
  assert.match(towerMsgs[0].text, /D-W1→A/);
});

test('SSE stream delivers state on mutation', async () => {
  const res = await fetch(url('/api/stream'));
  const reader = res.body.getReader();
  const dec = new TextDecoder();
  let buf = '';
  // initial frame
  while (!buf.includes('\n\n')) buf += dec.decode((await reader.read()).value);
  buf = '';
  await post('/api/card/add', { title: 'sse check' });
  const deadline = Date.now() + 3000;
  while (!buf.includes('sse check') && Date.now() < deadline) {
    const { value, done } = await reader.read();
    if (done) break;
    buf += dec.decode(value);
  }
  reader.cancel();
  assert.ok(buf.includes('sse check'), 'mutation broadcast arrived over SSE');
});
