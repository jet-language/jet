import { test } from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { watchForRestart } from '../app/restart.mjs';

function fakeServer() {
  return { closed: false, close(cb) { this.closed = true; cb(); }, closeAllConnections() {} };
}
function fakeChild() {
  const c = new EventEmitter();
  c.unref = () => { c.unrefed = true; };
  return c;
}

test('watchForRestart: child survives past the crash window → hands off (exits)', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'tower-restart-'));
  mkdirSync(join(dir, 'app'));
  let exitCode = null;
  let reopened = false;
  let spawnedChild = null;
  const { stop, restartNow } = watchForRestart({
    towerRoot: dir,
    argv: ['serve'],
    getServer: () => fakeServer(),
    reopen: () => { reopened = true; },
    log: () => {},
    spawnFn: () => (spawnedChild = fakeChild()),
    exitFn: (code) => { exitCode = code; },
    crashWindowMs: 30,
    debounceMs: 0,
  });
  await restartNow();
  assert.ok(spawnedChild, 'spawned a replacement process');
  assert.equal(exitCode, 0, 'hands off once the child outlives the crash window');
  assert.equal(reopened, false, 'no need to fall back — the new process is fine');
  assert.equal(spawnedChild.unrefed, true);
  stop();
});

test('watchForRestart: child crashes inside the window → falls back, does not exit', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'tower-restart-'));
  mkdirSync(join(dir, 'app'));
  let exitCode = null;
  let reopened = false;
  const { stop, restartNow } = watchForRestart({
    towerRoot: dir,
    argv: ['serve'],
    getServer: () => fakeServer(),
    reopen: () => { reopened = true; },
    log: () => {},
    spawnFn: () => {
      const c = fakeChild();
      setTimeout(() => c.emit('exit', 1), 5); // crashes almost immediately
      return c;
    },
    exitFn: (code) => { exitCode = code; },
    crashWindowMs: 200,
    debounceMs: 0,
  });
  await restartNow();
  assert.equal(exitCode, null, 'never hands off to a process that already died');
  assert.equal(reopened, true, 'falls back to reopening this process\'s own server');
  stop();
});

test('watchForRestart: fs change on a .mjs file triggers a restart (debounced)', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'tower-restart-'));
  mkdirSync(join(dir, 'app'));
  writeFileSync(join(dir, 'app', 'server.mjs'), 'export const x = 1;\n');
  let restarted = 0;
  const { stop } = watchForRestart({
    towerRoot: dir,
    argv: ['serve'],
    getServer: () => fakeServer(),
    reopen: () => {},
    log: () => {},
    spawnFn: () => { restarted++; return fakeChild(); },
    exitFn: () => {},
    crashWindowMs: 20,
    debounceMs: 10,
  });
  writeFileSync(join(dir, 'app', 'server.mjs'), 'export const x = 2;\n');
  await new Promise((r) => setTimeout(r, 200));
  assert.equal(restarted, 1, 'exactly one restart attempt for the change');
  stop();
});
