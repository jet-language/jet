import { test, after } from 'node:test';
import assert from 'node:assert/strict';
import { once } from 'node:events';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { empty, openStore } from '../app/store.mjs';
import { writeJSON } from '../app/paths.mjs';
import { serve } from '../app/server.mjs';

const root = mkdtempSync(join(tmpdir(), 'tower-gauntlet-'));
const dir = join(root, '.tower');
const statusPath = join(root, 'gauntlet', 'status.json');
mkdirSync(dir, { recursive: true });
mkdirSync(join(root, 'gauntlet'), { recursive: true });
writeJSON(join(dir, 'tower.json'), empty('Gauntlet route test'));
const store = openStore(dir);
const server = serve(store, 0, false);
await once(server, 'listening');
const port = server.address().port;
const get = () => fetch(`http://127.0.0.1:${port}/api/gauntlet`);

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
