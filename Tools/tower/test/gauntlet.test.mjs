// #6834: the Gauntlet route serves the gauntlet table
// ($JET_STATE_ROOT/gauntlet/table.json) over the grid planned from this
// checkout's entry manifests: a row the table holds is shown, every other
// measurable cell is not measured yet.
import { test, after } from 'node:test';
import assert from 'node:assert/strict';
import { once } from 'node:events';
import { mkdtempSync, mkdirSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { empty, openStore } from '../app/store.mjs';
import { writeJSON } from '../app/paths.mjs';
import { serve } from '../app/server.mjs';
import { readTable, recordEntry, tablePath, writeTable } from '../../gauntlet/harness/table.mjs';

const root = mkdtempSync(join(tmpdir(), 'tower-gauntlet-'));
// The machine's state root, holding the table, kept inside the test root.
const priorStateRoot = process.env.JET_STATE_ROOT;
process.env.JET_STATE_ROOT = join(root, 'state');
const dir = join(root, '.tower');
mkdirSync(dir, { recursive: true });
writeJSON(join(dir, 'tower.json'), empty('Gauntlet route test'));
const store = openStore(dir);
const server = serve(store, 0, false);
await once(server, 'listening');
const port = server.address().port;
const getStandings = () => fetch(`http://127.0.0.1:${port}/api/gauntlet/standings`);

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

test('with an empty table the route serves the planned grid from the entry manifests', async () => {
  const response = await getStandings();
  assert.equal(response.status, 200);
  const body = await response.json();
  assert.equal(body.file, join('state', 'gauntlet', 'table.json'));
  assert.deepEqual(body.table, { updated: null, entries: 0, rows: 0 });
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

test('each row the table holds shows on the route at once, with the table\'s counts', async () => {
  const table = await readTable(tablePath());
  recordEntry(table, { entry: { name: 'hello' }, rows: {}, standings_rows: { rust: { status: 'broken', reason: 'rust row reason' } } },
    { runId: 'run-1', generated: '2026-11-14T12:00:00.000Z', commit: 'abc', toolchains: {} });
  await writeTable(table, tablePath());
  const body = await (await getStandings()).json();
  assert.deepEqual(body.table, { updated: '2026-11-14T12:00:00.000Z', entries: 1, rows: 1 });
  const hello = body.standings.entries.find((entry) => entry.entry === 'hello');
  assert.deepEqual([hello.peers.rust.status, hello.peers.rust.reason], ['failed', 'rust row reason']);
  assert.deepEqual([hello.peers.c.status, hello.peers.c.reason], ['not_measured', 'not measured yet']);
  assert.deepEqual(hello.jet, { status: 'not_measured', reason: 'not measured yet' });
});
