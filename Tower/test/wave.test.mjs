import { test, after } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { openStore, empty } from '../app/store.mjs';
import { writeJSON } from '../app/paths.mjs';
import { serve } from '../app/server.mjs';

const dir = mkdtempSync(join(tmpdir(), 'tower-wave-'));
writeJSON(join(dir, 'tower.json'), empty('Wave'));
// opt-in auth token
writeJSON(join(dir, 'config.json'), { project: 'Wave', auth: { token: 'test-token-123456' } });
const store = openStore(dir);
const PORT = 7957;
const server = serve(store, PORT, false);
after(() => server.close());

const url = (p) => `http://localhost:${PORT}${p}`;
const post = async (p, b, raw = false) => {
  const r = await fetch(url(p), { method: 'POST', body: raw ? b : JSON.stringify(b) });
  return { status: r.status, json: await r.json().catch(() => null) };
};

test('provisioning: vapid keys generated; auth stays opt-in (no auto token)', () => {
  assert.ok(store.config.push?.publicKey?.length > 40);
  assert.equal(store.config.auth.token, 'test-token-123456', 'configured token respected, none invented');
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

test('retired routes are gone: files, agents, messages', async () => {
  const r1 = await fetch(url('/api/agents'));
  assert.equal(r1.status, 404);
  const r2 = await post('/api/message/send', { from: 'owner', to: 'a1', text: 'hi' });
  assert.equal(r2.status, 404);
  const r3 = await fetch(url('/api/file?name=x.png'), { method: 'POST', body: Buffer.from([1]) });
  assert.equal(r3.status, 404);
});

test('clearance batch ratifies without agent notifications', async () => {
  const { json: cardR } = await post('/api/card/add', { title: 'ballot host' });
  const cid = cardR.result.id;
  for (const n of [1, 2, 3]) await post('/api/decision/add', { cardId: cid, id: 'D-W' + n, title: 'w' + n,
    gist: 'g', story: 's', inWild: 'w', rec: 'A',
    options: [{ key: 'A', name: 'a', code: 'a()' }, { key: 'B', name: 'b', code: 'b()' }] });
  await post('/api/clearance', { decisionId: 'D-W1', outcome: 'A', by: 'owner' });
  await post('/api/clearance/batch', { by: 'owner', decisions: [{ decisionId: 'D-W2', outcome: 'A' }, { decisionId: 'D-W3', outcome: 'A' }] });
  const s = store.load();
  assert.equal(s.decisions.filter(d => d.status === 'ratified').length, 3);
  assert.equal(s.messages, undefined, 'no messages key survives');
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

// Non-localhost enforcement: reach the same server via the machine's real IP.
test('auth: remote requests 401 without key, unlock page for browsers, boot in state', async (t) => {
  const { networkInterfaces } = await import('node:os');
  const ip = Object.values(networkInterfaces()).flat().find(i => i && !i.internal && i.family === 'IPv4')?.address;
  if (!ip) return t.skip('no external interface');
  const base = `http://${ip}:${PORT}`;
  const r1 = await fetch(`${base}/api/state`);
  assert.equal(r1.status, 401);
  const r2 = await fetch(`${base}/`, { headers: { accept: 'text/html' } });
  assert.equal(r2.status, 401);
  assert.match(await r2.text(), /Unlock/);
  const r3 = await fetch(`${base}/api/state`, { headers: { authorization: `Bearer ${store.config.auth.token}` } });
  assert.equal(r3.status, 200);
  const s = await r3.json();
  assert.ok(s.boot?.length > 4, 'boot id present');
  const r4 = await fetch(`${base}/?key=${store.config.auth.token}`, { redirect: 'manual' });
  assert.equal(r4.status, 302);
  assert.match(r4.headers.get('set-cookie') || '', /tower=/);
});
