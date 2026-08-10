// #1738 — serve must never flush stale memory over newer CLI writes.
// Criterion 1: any board write through the server re-reads the store and
//   409s (instead of overwriting) when another writer advanced it.
// Criterion 2: the serve shutdown/handoff writes no state; the next
//   instance loads from disk.
// Criterion 3: a CLI write made during a serve session survives a serve
//   restart.
import { test, after } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { openStore, empty, addCard } from '../app/store.mjs';
import { configFile, writeJSON } from '../app/paths.mjs';
import { serve } from '../app/server.mjs';

const dir = mkdtempSync(join(tmpdir(), 'tower-flush-'));
writeJSON(join(dir, 'tower.json'), empty('Flush'));
writeJSON(configFile(dir), { project: 'Flush' });

const PORT_A = 7961;
const PORT_B = 7962;
let serverB = null;
after(() => serverB?.close());

const post = async (port, route, body) => {
  const r = await fetch(`http://localhost:${port}/api/${route}`, { method: 'POST', body: JSON.stringify(body) });
  return { status: r.status, json: await r.json() };
};

test('#1738: CLI writes behind a serve session are refused-then-honored, never overwritten, and survive a restart', async () => {
  const storeA = openStore(dir);
  const serverA = serve(storeA, PORT_A, false);

  // a normal write through the server works and the server has seen its rev
  const viaServer = await post(PORT_A, 'card/add', { title: 'via server', by: 'ui' });
  assert.equal(viaServer.status, 200);

  // a CLI writer (separate store handle, same data dir) advances the store
  // behind the server's back — exactly the incident's shape
  const cli = openStore(dir);
  cli.mutate((s, cfg) => addCard(s, { title: 'via cli', by: 'cli-agent' }, cfg));

  // criterion 1: the next server write is refused with a conflict …
  const refused = await post(PORT_A, 'card/update', { id: '#1', title: 'renamed by ui' });
  assert.equal(refused.status, 409, 'server write after an external advance must 409, not overwrite');
  assert.equal(refused.json.error, 'E_CONFLICT');

  // … and the retry (server has now caught up) succeeds without losing the CLI write
  const retried = await post(PORT_A, 'card/update', { id: '#1', title: 'renamed by ui' });
  assert.equal(retried.status, 200);
  assert.ok(retried.json.state.cards.some(c => c.title === 'via cli'), 'CLI write still present after server write');

  // criterion 2: shutting the server down (the handoff's only server-side
  // step) writes no state — the store file is byte-identical across it
  const before = readFileSync(join(dir, 'tower.json'), 'utf8');
  await new Promise((resolve) => { serverA.close(() => resolve()); serverA.closeAllConnections?.(); });
  const afterClose = readFileSync(join(dir, 'tower.json'), 'utf8');
  assert.equal(afterClose, before, 'serve shutdown must not write state');

  // criterion 3: a fresh instance loads from disk and both writes survived
  const storeB = openStore(dir);
  serverB = serve(storeB, PORT_B, false);
  const state = await (await fetch(`http://localhost:${PORT_B}/api/state`)).json();
  const titles = state.cards.map(c => c.title).sort();
  assert.deepEqual(titles, ['renamed by ui', 'via cli']);

  // and the new instance accepts writes on top of the surviving state
  const more = await post(PORT_B, 'card/add', { title: 'after restart', by: 'ui' });
  assert.equal(more.status, 200);
  assert.equal(more.json.state.cards.length, 3);
});

test('#1738: whole-board replaces require rev proof', async () => {
  // /api/undo with no expectRev is refused before it can touch the store
  const bare = await post(PORT_B, 'undo', {});
  assert.equal(bare.status, 400);
  assert.equal(bare.json.error, 'E_USAGE');

  // the store chokepoint enforces the same law for every caller
  const cli = openStore(dir);
  assert.throws(() => cli.restore(empty('Flush')), (e) => e.code === 'E_USAGE');
});
