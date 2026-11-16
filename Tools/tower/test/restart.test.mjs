import { test } from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { superviseServe, serveWorker } from '../app/restart.mjs';

// A fake `tower serve --worker` child. `behavior` decides how it answers the
// supervisor: 'ok' listens and releases, 'crash' dies during boot, 'hang'
// never listens.
function fakeWorker(behavior = 'ok') {
  const c = new EventEmitter();
  c.sent = [];
  c.killed = null;
  c.send = (m) => {
    c.sent.push(m.tower);
    if (m.tower === 'release') setImmediate(() => c.emit('message', { tower: 'released' }));
    if (m.tower === 'reopen') setImmediate(() => c.emit('message', { tower: 'listening' }));
  };
  c.kill = (signal) => { c.killed = signal; setImmediate(() => c.emit('exit', null, signal)); };
  setImmediate(() => {
    if (behavior === 'ok') c.emit('message', { tower: 'listening' });
    if (behavior === 'crash') c.emit('exit', 1, null);
  });
  return c;
}

function supervise(dir, behaviors, extra = {}) {
  const spawned = [];
  const exits = [];
  const sup = superviseServe({
    towerRoot: dir,
    argv: ['serve', '--open'],
    log: () => {},
    spawnFn: (_exec, args) => {
      const w = fakeWorker(behaviors[spawned.length] ?? 'ok');
      w.args = args;
      spawned.push(w);
      return w;
    },
    exitFn: (code) => exits.push(code),
    debounceMs: 0,
    readyTimeoutMs: 50,
    releaseTimeoutMs: 50,
    handleSignals: false,
    ...extra,
  });
  return { sup, spawned, exits };
}

function board() {
  const dir = mkdtempSync(join(tmpdir(), 'tower-restart-'));
  mkdirSync(join(dir, 'app'));
  return dir;
}

test('superviseServe: the supervisor stays alive and swaps workers once the new one listens', async () => {
  const { sup, spawned, exits } = supervise(board(), ['ok', 'ok']);
  await sup.restartNow();
  assert.equal(spawned.length, 2, 'started a replacement worker');
  assert.deepEqual(spawned[0].sent, ['release'], 'the old worker released its port first');
  assert.equal(spawned[0].killed, 'SIGTERM', 'the old worker stops only after the new one listens');
  assert.equal(sup.worker().child, spawned[1]);
  assert.deepEqual(exits, [], 'the supervisor never exits on a successful restart');
  assert.ok(spawned[0].args.includes('--open') && spawned[0].args.includes('--worker'));
  assert.ok(!spawned[1].args.includes('--open'), 'restarts never open another browser');
  sup.stop();
});

test('superviseServe: a replacement that crashes on boot falls back to the old worker', async () => {
  const { sup, spawned, exits } = supervise(board(), ['ok', 'crash']);
  await sup.restartNow();
  assert.deepEqual(spawned[0].sent, ['release', 'reopen'], 'the old worker reopens its server');
  assert.equal(spawned[0].killed, null);
  assert.equal(sup.worker().child, spawned[0]);
  assert.deepEqual(exits, []);
  sup.stop();
});

test('superviseServe: a replacement that never listens is stopped before the old worker reopens', async () => {
  const { sup, spawned, exits } = supervise(board(), ['ok', 'hang']);
  await sup.restartNow();
  assert.equal(spawned[1].killed, 'SIGTERM');
  assert.deepEqual(spawned[0].sent, ['release', 'reopen']);
  assert.deepEqual(exits, []);
  sup.stop();
});

test('superviseServe: the serving worker dying outside a restart exits the supervisor with its code', async () => {
  const { sup, spawned, exits } = supervise(board(), ['ok']);
  await new Promise((r) => setImmediate(r));
  spawned[0].emit('exit', 3, null);
  assert.deepEqual(exits, [3]);
  sup.stop();
});

test('superviseServe: an fs change on a .mjs file triggers one restart (debounced)', async () => {
  const dir = board();
  writeFileSync(join(dir, 'app', 'server.mjs'), 'export const x = 1;\n');
  const { sup, spawned } = supervise(dir, ['ok', 'ok'], { debounceMs: 10 });
  writeFileSync(join(dir, 'app', 'server.mjs'), 'export const x = 2;\n');
  await new Promise((r) => setTimeout(r, 200));
  assert.equal(spawned.length, 2, 'exactly one restart for the change');
  sup.stop();
});

test('serveWorker: reports listening, releases and reopens on request, exits when the supervisor goes', async () => {
  const channel = new EventEmitter();
  const sent = [];
  channel.send = (m) => sent.push(m.tower);
  const servers = [];
  const exits = [];
  serveWorker({
    serveFn: (open) => {
      const s = new EventEmitter();
      s.open = open;
      s.close = (cb) => setImmediate(cb);
      s.closeAllConnections = () => { s.forced = true; };
      servers.push(s);
      setImmediate(() => s.emit('listening'));
      return s;
    },
    open: true,
    channel,
    exitFn: (code) => exits.push(code),
  });
  await new Promise((r) => setImmediate(r));
  assert.deepEqual(sent, ['listening']);
  channel.emit('message', { tower: 'release' });
  await new Promise((r) => setImmediate(r));
  assert.equal(servers[0].forced, true, 'keep-alive SSE sockets are closed with the listener');
  assert.deepEqual(sent, ['listening', 'released']);
  channel.emit('message', { tower: 'reopen' });
  await new Promise((r) => setImmediate(r));
  assert.equal(servers.length, 2);
  assert.equal(servers[1].open, false, 'a reopen never opens a browser');
  assert.deepEqual(sent, ['listening', 'released', 'listening']);
  channel.emit('disconnect');
  assert.deepEqual(exits, [0]);
});
