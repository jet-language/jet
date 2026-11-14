// #6834: the Gauntlet route serves one run: the newest full or merged results
// file under config gauntletResults (default Tools/gauntlet/results) and the
// shared results directory, else the planned grid from this checkout's entry
// manifests. A partial run is never served.
import { test, after } from 'node:test';
import assert from 'node:assert/strict';
import { once } from 'node:events';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { empty, openStore } from '../app/store.mjs';
import { writeJSON } from '../app/paths.mjs';
import { serve } from '../app/server.mjs';

const root = mkdtempSync(join(tmpdir(), 'tower-gauntlet-'));
// The machine's shared results directory, kept inside the test root.
const priorStateRoot = process.env.JET_STATE_ROOT;
process.env.JET_STATE_ROOT = join(root, 'state');
const sharedDir = join(root, 'state', 'gauntlet', 'results');
const dir = join(root, '.tower');
mkdirSync(dir, { recursive: true });
writeJSON(join(dir, 'tower.json'), empty('Gauntlet route test'));
const store = openStore(dir);
const server = serve(store, 0, false);
await once(server, 'listening');
const port = server.address().port;
const getStandings = () => fetch(`http://127.0.0.1:${port}/api/gauntlet/standings`);
const resultsDir = join(root, 'Tools', 'gauntlet', 'results');
const fixture = (name) => JSON.parse(readFileSync(new URL(`./fixtures/${name}`, import.meta.url), 'utf8'));

after(() => {
  server.close();
  if (priorStateRoot === undefined) delete process.env.JET_STATE_ROOT;
  else process.env.JET_STATE_ROOT = priorStateRoot;
  rmSync(root, { recursive: true, force: true });
});

test('the old status matrix route is gone', async () => {
  const response = await fetch(`http://127.0.0.1:${port}/api/gauntlet`);
  assert.equal(response.status, 404);
});

test('before any full run the route serves the planned grid from the entry manifests', async () => {
  mkdirSync(resultsDir, { recursive: true });
  // A peers-only run is partial: it is never shown.
  writeFileSync(join(resultsDir, '2026-11-14-languages-rust-c.json'), `${JSON.stringify({ ...fixture('gauntlet-results-peers.json'), options: { scope: 'partial_entry' } })}\n`);
  const response = await getStandings();
  assert.equal(response.status, 200);
  const body = await response.json();
  assert.equal(body.scope, 'planned');
  assert.equal(body.file, null);
  assert.equal(body.run, null);
  const { standings } = body;
  assert.deepEqual(standings.peers, ['rust', 'c', 'zig', 'go', 'python', 'node', 'bun', 'vite', 'lua', 'luajit']);
  assert.equal(standings.ceiling, 'asm');
  assert.ok(standings.entries.length > 20);
  for (const entry of standings.entries) {
    assert.deepEqual(entry.jet, { status: 'not_measured', reason: 'not measured yet' });
    assert.deepEqual(Object.keys(entry.peers), [...standings.peers, 'asm']);
    for (const item of Object.values(entry.peers)) {
      assert.ok(['not_measured', 'uncovered', 'not_applicable', 'failed'].includes(item.status), `${entry.entry}: ${item.status}`);
      assert.ok(item.reason, `${entry.entry}: every cell names its state`);
      for (const cell of Object.values(item.metrics)) assert.deepEqual([cell.jet, cell.peer, cell.ratio], [null, null, null]);
    }
  }
  const hello = standings.entries.find((entry) => entry.entry === 'hello');
  assert.equal(hello.peers.rust.status, 'not_measured');
  assert.equal(hello.peers.lua.status, 'not_measured');
  assert.equal(hello.peers.luajit.program, 'lua/main.lua');
  assert.equal(hello.peers.vite.status, 'not_applicable');
  assert.equal(hello.peers.asm.role, 'ceiling');
  assert.equal(hello.peers.asm.reference, 'hello');
  assert.ok(standings.entries.some((entry) => entry.peers.asm.status === 'uncovered'), 'an entry with no asm reference keeps its asm cell');
  assert.ok(standings.entries.some((entry) => entry.peers.lua.status === 'uncovered' && /^no lua program: lua\/main\.lua is missing$/.test(entry.peers.lua.reason)));
});

test('the newest full or merged run replaces the planned grid, and names its date and commit', async () => {
  const full = { ...fixture('gauntlet-results.json'), options: { scope: 'full_matrix' }, run_id: 'run-full', generated: '2026-11-13T10:00:00.000Z', provenance: { commit: 'aaaaaaaaaaaaaaaa' } };
  writeFileSync(join(resultsDir, '2026-11-13.json'), `${JSON.stringify(full)}\n`);
  let body = await (await getStandings()).json();
  assert.equal(body.file, join('Tools', 'gauntlet', 'results', '2026-11-13.json'));
  assert.equal(body.scope, 'full_matrix');
  assert.deepEqual(body.run, { id: 'run-full', generated: '2026-11-13T10:00:00.000Z', commit: 'aaaaaaaaaaaaaaaa', merged_from: null });

  const merged = {
    ...fixture('gauntlet-results-peers.json'),
    options: { scope: 'full_matrix' },
    run_id: 'run-merged',
    generated: '2026-11-14T09:30:00.000Z',
    merged_from: { run_id: 'run-peers', generated: '2026-11-14T01:00:00.000Z', commit: 'bbbbbbbbbbbb' },
    provenance: { commit: 'cccccccccccc' },
  };
  mkdirSync(sharedDir, { recursive: true });
  writeFileSync(join(sharedDir, '2026-11-14-merged-languages-asm.json'), `${JSON.stringify(merged)}\n`);
  body = await (await getStandings()).json();
  assert.equal(body.file, join('state', 'gauntlet', 'results', '2026-11-14-merged-languages-asm.json'));
  assert.equal(body.standings.entries[0].jet.status, 'not_measured');
  assert.deepEqual(body.run, { id: 'run-merged', generated: '2026-11-14T09:30:00.000Z', commit: 'cccccccccccc', merged_from: { id: 'run-peers', generated: '2026-11-14T01:00:00.000Z', commit: 'bbbbbbbbbbbb' } });
});
