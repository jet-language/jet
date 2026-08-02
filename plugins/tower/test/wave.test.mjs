import { test, after } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { openStore, empty } from '../app/store.mjs';
import { saveSecrets } from '../app/config.mjs';
import { configFile, readJSON, secretsFile, writeJSON } from '../app/paths.mjs';
import { serve } from '../app/server.mjs';

const dir = mkdtempSync(join(tmpdir(), 'tower-wave-'));
writeJSON(join(dir, 'tower.json'), empty('Wave'));
// opt-in auth token
writeJSON(configFile(dir), { project: 'Wave' });
saveSecrets(dir, { auth: { token: 'test-token-123456' } });
const store = openStore(dir);
const PORT = 7957;
const server = serve(store, PORT, false);
after(() => server.close());

const url = (p) => `http://localhost:${PORT}${p}`;
const post = async (p, b, raw = false) => {
  const r = await fetch(url(p), { method: 'POST', body: raw ? b : JSON.stringify(b) });
  return { status: r.status, json: await r.json().catch(() => null) };
};

test('provisioning: no vapid; auth stays opt-in (no auto token)', () => {
  assert.equal(store.config.push, null);
  assert.equal(store.config.auth.token, 'test-token-123456', 'configured token respected, none invented');
  assert.deepEqual(Object.keys(readJSON(configFile(dir), {})), ['project']);
  const secrets = readJSON(secretsFile(dir), {});
  assert.equal(Object.hasOwn(secrets, 'push'), false);
  assert.equal(typeof secrets.auth?.token, 'string');
  const projected = store.project();
  assert.equal(Object.hasOwn(projected.config, 'auth'), false);
  assert.equal(Object.hasOwn(projected.config, 'push'), false);
});

test('push routes are gone', async () => {
  const key = await fetch(url('/api/push/key'));
  assert.equal(key.status, 404);
  const sub = await post('/api/push/subscribe', { subscription: { endpoint: 'https://example.invalid/x', keys: {} } });
  assert.equal(sub.status, 404);
  const testPush = await post('/api/push/test', {});
  assert.equal(testPush.status, 404);
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
    ballotMode: 'full', reviewPasses: { base: 'The base pass completed the ballot.', boilOcean: 'The boil-the-ocean pass tested the broad solution space.', hybrid: 'The hybrid pass combined compatible strengths.', cooperative: 'The cooperative pass strengthened each option.', adversarial: 'The adversarial pass attacked the recommendation.' },
    gist: 'g', lesson: 'teach from zero', story: 's', inWild: 'w', rec: 'A',
    recommendation: { why: 'A wins here.', whyNot: [{ key: 'B', reason: 'B loses the needed behavior.' }], tradeoff: 'A adds one visible step.' },
    hybrid: { result: 'A', synthesis: 'A combines the useful parts.', harvest: [{ key: 'A', aspect: 'A is explicit.', use: 'Keep it.' }, { key: 'B', aspect: 'B is brief.', use: 'Borrow its short names.' }] },
    options: [{ key: 'A', name: 'a', detail: 'A is explicit.', code: 'a()' }, { key: 'B', name: 'b', detail: 'B is brief.', code: 'b()' }] });
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
  const initial = JSON.parse(buf.split('data: ')[1].split('\n\n')[0]);
  assert.equal(Object.hasOwn(initial.config, 'auth'), false);
  assert.equal(Object.hasOwn(initial.config, 'push'), false);
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
  assert.equal(Object.hasOwn(s.config, 'auth'), false);
  assert.equal(Object.hasOwn(s.config, 'push'), false);
  const r4 = await fetch(`${base}/?key=${store.config.auth.token}`, { redirect: 'manual' });
  assert.equal(r4.status, 302);
  assert.match(r4.headers.get('set-cookie') || '', /tower=/);
});
