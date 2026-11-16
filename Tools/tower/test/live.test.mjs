// Live board: in-memory index, index deltas over SSE, and field-level owner
// writes while agents write the same board through the CLI.
import { test, after } from 'node:test';
import assert from 'node:assert/strict';
import { once } from 'node:events';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import * as db from '../app/store.mjs';
import { configFile, writeJSON } from '../app/paths.mjs';
import { serve } from '../app/server.mjs';
import { diffIndex, applyIndexDelta } from '../app/ui/live-delta.js';
import { withLockAsync } from '../app/lock.mjs';

test('index deltas round-trip: keyed upserts, removals, reorders, log heads, and replaced sections', () => {
  const a = { id: 'a', n: 1 }, b = { id: 'b', n: 2 }, c = { id: 'c', n: 3 };
  const prev = {
    meta: { rev: 1 }, cards: [a, b, c], events: [{ at: '2', action: 'y' }, { at: '1', action: 'x' }],
    counts: { open: 3 }, statusSnapshot: null, gone: true,
  };
  const next = {
    meta: { rev: 2 }, cards: [{ id: 'c', n: 30 }, a, { id: 'd', n: 4 }],
    events: [{ at: '3', action: 'z' }, { at: '2', action: 'y' }], counts: { open: 3 }, statusSnapshot: { id: 's1' },
  };
  const patch = diffIndex(prev, next);
  assert.deepEqual(applyIndexDelta(prev, patch), next);
  assert.deepEqual(patch.lists.cards.remove, ['b']);
  assert.deepEqual(patch.lists.cards.upsert.map(x => x.id), ['c', 'd']);
  assert.deepEqual(patch.heads.events, { prepend: [{ at: '3', action: 'z' }], length: 2 });
  assert.equal('counts' in (patch.set || {}), false, 'unchanged sections are not resent');
  assert.equal(patch.set.gone, null);
  // untouched records keep their identity on the client
  assert.equal(applyIndexDelta(prev, patch).cards[1], a);
  assert.deepEqual(diffIndex(next, next), {});
});


test('server writes wait for a held board lock without blocking the event loop', async () => {
  const lockDir = mkdtempSync(join(tmpdir(), 'tower-lock-'));
  const file = join(lockDir, 'tower.json');
  mkdirSync(`${file}.lock`);
  writeFileSync(join(`${file}.lock`, 'info.json'), JSON.stringify({ pid: process.pid, at: Date.now() }));
  let ticks = 0;
  const ticker = setInterval(() => { ticks++; }, 10);
  setTimeout(() => rmSync(`${file}.lock`, { recursive: true, force: true }), 150);
  const result = await withLockAsync(file, () => 'wrote');
  clearInterval(ticker);
  assert.equal(result, 'wrote');
  assert.ok(ticks >= 5, `timers kept running while waiting (${ticks})`);
  rmSync(lockDir, { recursive: true, force: true });
});
const dir = mkdtempSync(join(tmpdir(), 'tower-live-'));
writeJSON(join(dir, 'tower.json'), db.empty('Live'));
writeJSON(configFile(dir), { project: 'Live' });
const server = serve(db.openStore(dir), 0, false);
await once(server, 'listening');
after(async () => {
  server.closeAllConnections?.();
  await new Promise(resolve => server.close(resolve));
  rmSync(dir, { recursive: true, force: true });
});
const url = (p) => `http://localhost:${server.address().port}${p}`;
const post = async (route, body) => {
  const r = await fetch(url(`/api/${route}`), {
    method: 'POST', headers: { 'content-type': 'application/json', 'x-tower-client': 'cli' }, body: JSON.stringify(body),
  });
  return { status: r.status, json: await r.json() };
};

// Minimal SSE reader: yields {event, data} frames.
async function* frames(response) {
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';
  for (;;) {
    let at;
    while ((at = buffer.indexOf('\n\n')) >= 0) {
      const raw = buffer.slice(0, at);
      buffer = buffer.slice(at + 2);
      const event = /^event: (.*)$/m.exec(raw)?.[1];
      const data = /^data: (.*)$/m.exec(raw)?.[1];
      if (event) yield { event, data: JSON.parse(data) };
    }
    const { value, done } = await reader.read();
    if (done) return;
    buffer += decoder.decode(value, { stream: true });
  }
}

test('a client holding the index resumes with deltas only, and CLI writes arrive as deltas that rebuild /api/state', async () => {
  await post('card/add', { title: 'first', by: 'agent-a' });
  let index = await (await fetch(url('/api/state'))).json();
  const ctl = new AbortController();
  const stream = await fetch(url(`/api/stream?rev=${index.meta.rev}&boot=${index.boot}`), { signal: ctl.signal });
  const events = frames(stream);
  const hello = (await events.next()).value;
  assert.equal(hello.event, 'hello', 'an up-to-date client gets no full index');
  assert.equal(hello.data.rev, index.meta.rev);

  // An agent writes through its own store handle (the CLI path), not HTTP.
  const cli = db.openStore(dir);
  cli.mutate((s, cfg) => db.addCard(s, { title: 'from the cli', by: 'agent-b' }, cfg));
  const delta = (await events.next()).value;
  assert.equal(delta.event, 'delta');
  assert.equal(delta.data.base, index.meta.rev);
  assert.ok(JSON.stringify(delta.data.patch).length < 4000, 'a delta carries the change, not the board');
  index = applyIndexDelta(index, delta.data.patch);
  const fresh = await (await fetch(url('/api/state'))).json();
  assert.deepEqual(index, fresh);
  assert.ok(index.cards.some(c => c.title === 'from the cli'));
  ctl.abort();

  // A client whose rev the ring cannot bridge gets the full index.
  const ctl2 = new AbortController();
  const stale = frames(await fetch(url(`/api/stream?rev=0&boot=${index.boot}`), { signal: ctl2.signal }));
  assert.equal((await stale.next()).value.event, 'state');
  ctl2.abort();
});

test('owner field edits land after unrelated agent writes without a conflict, and card detail reads stay current', async () => {
  const { json } = await post('card/add', { title: 'shared card', by: 'agent-a' });
  const ref = `#${json.result.num}`;
  const cli = db.openStore(dir);
  cli.mutate((s, cfg) => db.updateCard(s, ref, { priority: 'P0', by: 'agent-b' }, cfg));
  cli.mutate((s, cfg) => db.addCard(s, { title: 'other work', by: 'agent-c' }, cfg));
  const owner = await post('card/update', { id: ref, title: 'owner title', by: 'owner' });
  assert.equal(owner.status, 200, JSON.stringify(owner.json));
  const detail = (await (await fetch(url(`/api/card?id=${encodeURIComponent(ref)}`))).json()).card;
  assert.equal(detail.title, 'owner title');
  assert.equal(detail.priority, 'P0', 'the agent field edit survives');
  // A later CLI edit is visible to the next detail read without a restart.
  cli.mutate((s, cfg) => db.updateCard(s, ref, { body: 'agent body', by: 'agent-b' }, cfg));
  const again = (await (await fetch(url(`/api/card?id=${encodeURIComponent(ref)}`))).json()).card;
  assert.equal(again.body, 'agent body');
  assert.equal(again.title, 'owner title');
});
