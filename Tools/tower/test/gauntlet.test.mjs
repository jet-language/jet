import { test, after } from 'node:test';
import assert from 'node:assert/strict';
import { once } from 'node:events';
import { copyFileSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { empty, openStore } from '../app/store.mjs';
import { writeJSON } from '../app/paths.mjs';
import { serve } from '../app/server.mjs';

const root = mkdtempSync(join(tmpdir(), 'tower-gauntlet-'));
const dir = join(root, '.tower');
const statusPath = join(root, 'Tools', 'gauntlet', 'status.json');
mkdirSync(dir, { recursive: true });
mkdirSync(join(root, 'Tools', 'gauntlet'), { recursive: true });
writeJSON(join(dir, 'tower.json'), empty('Gauntlet route test'));
const store = openStore(dir);
const server = serve(store, 0, false);
await once(server, 'listening');
const port = server.address().port;
const get = () => fetch(`http://127.0.0.1:${port}/api/gauntlet`);
const getStandings = () => fetch(`http://127.0.0.1:${port}/api/gauntlet/standings`);
const resultsDir = join(root, 'Tools', 'gauntlet', 'results');
const fixtureResults = new URL('./fixtures/gauntlet-results.json', import.meta.url);

after(() => {
  server.close();
  rmSync(root, { recursive: true, force: true });
});

test('Gauntlet route reads the tracked fixture on demand', async () => {
  const fixture = {
    contract: 'gauntlet-status-v1',
    generated: '2026-09-02T00:00:00.000Z',
    summary: { cells: 1, loss: 1 },
  };
  writeFileSync(statusPath, `${JSON.stringify(fixture)}\n`);
  const response = await get();
  assert.equal(response.status, 200);
  assert.deepEqual(await response.json(), fixture);
});

test('Gauntlet route reports a clear 404 when status is absent', async () => {
  rmSync(statusPath, { force: true });
  const response = await get();
  assert.equal(response.status, 404);
  const body = await response.json();
  assert.equal(body.error, 'E_NOT_FOUND');
  assert.match(body.message, /Gauntlet status is unavailable/);
});

// #6834: the standings route serves the newest results file under config
// gauntletResults (default Tools/gauntlet/results), preferring a full run.
test('Standings route reports a clear 404 before any results exist', async () => {
  const response = await getStandings();
  assert.equal(response.status, 404);
  const body = await response.json();
  assert.equal(body.error, 'E_NOT_FOUND');
  assert.match(body.message, /Gauntlet standings are unavailable/);
});

test('Standings route serves the newest results file and prefers a full run', async () => {
  mkdirSync(resultsDir, { recursive: true });
  copyFileSync(fixtureResults, join(resultsDir, '2026-11-14-hello.json'));
  writeFileSync(join(resultsDir, '2026-11-15-axis-live_reload.json'), '{"contract":"gauntlet-report-v1","options":{"scope":"axis_live_reload"}}\n');
  let response = await getStandings();
  assert.equal(response.status, 200);
  let body = await response.json();
  assert.equal(body.file, join('Tools', 'gauntlet', 'results', '2026-11-14-hello.json'));
  assert.equal(body.scope, 'partial_entry');
  assert.deepEqual(Object.keys(body.standings.entries[0].peers), ['rust', 'c', 'zig', 'go', 'python', 'bun', 'node', 'vite', 'asm']);

  const full = JSON.parse(readFileSync(fixtureResults, 'utf8'));
  full.options = { scope: 'full_matrix' };
  writeFileSync(join(resultsDir, '2026-11-13.json'), `${JSON.stringify(full)}\n`);
  response = await getStandings();
  body = await response.json();
  assert.equal(body.file, join('Tools', 'gauntlet', 'results', '2026-11-13.json'));
  assert.equal(body.scope, 'full_matrix');
});
