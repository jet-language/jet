import { test, after } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import * as db from '../app/store.mjs';
import { openStore, empty } from '../app/store.mjs';
import { writeJSON } from '../app/paths.mjs';
import { serve } from '../app/server.mjs';

const fresh = () => {
  const dir = mkdtempSync(join(tmpdir(), 'tower-msg-'));
  writeJSON(join(dir, 'tower.json'), empty('Msg'));
  return openStore(dir);
};

test('message store: send, threads, delivered/read marks', () => {
  const st = fresh();
  st.mutate((s) => db.sendMessage(s, { from: 'owner', to: 'claude-main', text: 'status?' }));
  st.mutate((s) => db.sendMessage(s, { from: 'claude-main', to: 'owner', text: 'building #4' }));
  let s = st.load();
  assert.equal(s.messages.length, 2);
  const ts = db.threads(s);
  assert.equal(ts.length, 1);
  assert.equal(ts[0].agent, 'claude-main');
  assert.equal(ts[0].unreadForOwner, 1);

  const pending = db.pendingFor(s, 'claude-main');
  assert.equal(pending.length, 1);
  st.mutate((s2) => db.markMessages(s2, pending.map(m => m.id), 'deliveredAt'));
  assert.equal(db.pendingFor(st.load(), 'claude-main').length, 0);

  assert.equal(db.project(st.load()).counts.unreadForOwner, 1);
  const ids = st.load().messages.filter(m => m.to === 'owner').map(m => m.id);
  st.mutate((s2) => db.markMessages(s2, ids, 'readAt'));
  assert.equal(db.project(st.load()).counts.unreadForOwner, 0);

  assert.throws(() => st.mutate((s2) => db.sendMessage(s2, { from: 'owner', to: 'x', text: '' })), db.TowerError);
  assert.throws(() => st.mutate((s2) => db.sendMessage(s2, { from: 'owner', text: 'no target' })), db.TowerError);
});

test('agent reply without --to defaults to owner', () => {
  const st = fresh();
  const { result } = st.mutate((s) => db.sendMessage(s, { by: 'codex-1', text: 'done' }));
  assert.equal(result.to, 'owner');
  assert.equal(result.from, 'codex-1');
});

// server: long-poll delivers instantly when a message is already pending, and
// wakes a held poll when one arrives via message/send
const dir2 = mkdtempSync(join(tmpdir(), 'tower-msgsrv-'));
writeJSON(join(dir2, 'tower.json'), empty('MsgSrv'));
const store2 = openStore(dir2);
const PORT = 7956;
const server = serve(store2, PORT, false);
after(() => server.close());

test('server long-poll: pending → instant; held poll wakes on send', async () => {
  store2.mutate((s) => db.sendMessage(s, { from: 'owner', to: 'a1', text: 'first' }));
  const r1 = await fetch(`http://localhost:${PORT}/api/messages/wait?for=a1&kind=claude`);
  const batch1 = await r1.json();
  assert.equal(batch1.length, 1);
  assert.equal(batch1[0].text, 'first');
  // marked delivered → immediate re-poll would hold; instead test wake-on-send
  const held = fetch(`http://localhost:${PORT}/api/messages/wait?for=a1`);
  await new Promise(r => setTimeout(r, 150));
  await fetch(`http://localhost:${PORT}/api/message/send`, { method: 'POST', body: JSON.stringify({ from: 'owner', to: 'a1', text: 'second' }) });
  const batch2 = await (await held).json();
  assert.equal(batch2.length, 1);
  assert.equal(batch2[0].text, 'second');

  // presence: a1 seen as listener
  const roster = await (await fetch(`http://localhost:${PORT}/api/agents`)).json();
  const a1 = roster.find(a => a.name === 'a1');
  assert.ok(a1 && a1.online);
  assert.equal(a1.kind, 'claude');
});

test('launch bridge rejects when no command configured', async () => {
  const r = await fetch(`http://localhost:${PORT}/api/agent/launch`, { method: 'POST', body: JSON.stringify({ agent: 'a1', text: 'go' }) });
  assert.equal(r.status, 400);
  const j = await r.json();
  assert.match(j.message, /no launch command/);
});
