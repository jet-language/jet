// Self-restart on source change (#522). `tower serve` loads all its
// route/db code once at process start (see server.mjs): if an agent edits
// that code and nobody restarts the process, the served UI (read fresh off
// disk every request) looks current while the running process is still the
// old one, so new endpoints 404 forever.
//
// Design: plain `tower serve` is a small supervisor that holds no board
// state. The HTTP server runs in a worker child (`tower serve --worker`)
// that shares the supervisor's stdio and talks to it over IPC. On a change
// to Tower's own .mjs source the supervisor asks the running worker to
// release the port, starts a fresh worker, and waits until that worker
// reports it is listening; only then does it stop the old worker. If the new
// worker dies or never listens, the old worker reopens its server and keeps
// serving the code it loaded: no restart loop, and the version-mismatch
// banner (version.mjs + server.mjs's /api/version) tells the owner a manual
// fix and restart are needed.
//
// The supervisor's PID and stdio stay the same for the whole session, so
// whatever launched `tower serve` (a terminal, a service manager) keeps
// tracking the live server. The previous design spawned a detached
// replacement and exited 0; the launcher then saw a clean exit and closed
// the terminal the replacement had inherited, and the orphaned replacement
// died on its next log write (2026-10-04 21:0x: "new process is up —
// handing off", then nothing listening on the port).
import { watch } from 'node:fs';
import { spawn as defaultSpawn } from 'node:child_process';
import { join } from 'node:path';

const DEBOUNCE_MS = 300;
// Boot loads and projects the whole board before listening.
const READY_TIMEOUT_MS = 60_000;
const RELEASE_TIMEOUT_MS = 5_000;

// towerRoot: dir containing tower.mjs. argv: process.argv.slice(2) (the
// original CLI args; each worker runs the identical command plus --worker).
export function superviseServe({
  towerRoot,
  argv,
  log = (...a) => console.log(...a),
  spawnFn = defaultSpawn,
  exitFn = (code) => process.exit(code),
  debounceMs = DEBOUNCE_MS,
  readyTimeoutMs = READY_TIMEOUT_MS,
  releaseTimeoutMs = RELEASE_TIMEOUT_MS,
  handleSignals = true,
}) {
  let current = null;
  let timer = null;
  let inFlight = false;
  let stopping = false;

  function start(args) {
    const child = spawnFn(process.execPath, [process.argv[1], ...args, '--worker'], { stdio: ['inherit', 'inherit', 'inherit', 'ipc'] });
    const worker = { child, exited: false, code: null };
    child.on('exit', (code, signal) => {
      worker.exited = true;
      worker.code = code ?? (signal ? 1 : 0);
      // Outside a restart, the serving worker's death is the server's death:
      // the supervisor exits with its code so the launcher sees it.
      if (worker === current && (!inFlight || stopping)) exitFn(worker.code);
    });
    return worker;
  }

  function send(worker, tower) {
    if (worker.exited) return;
    try { worker.child.send({ tower }); } catch { /* channel already closed */ }
  }

  // Resolves true on the worker's `type` message, false on exit or timeout.
  function waitFor(worker, type, ms) {
    return new Promise((resolve) => {
      if (worker.exited) return resolve(false);
      const done = (ok) => {
        clearTimeout(t);
        worker.child.off('message', onMessage);
        worker.child.off('exit', onExit);
        resolve(ok);
      };
      const onMessage = (m) => { if (m && m.tower === type) done(true); };
      const onExit = () => done(false);
      const t = setTimeout(() => done(false), ms);
      worker.child.on('message', onMessage);
      worker.child.on('exit', onExit);
    });
  }

  function stopWorker(worker) {
    return new Promise((resolve) => {
      if (worker.exited) return resolve();
      worker.child.once('exit', () => resolve());
      worker.child.kill('SIGTERM');
    });
  }

  async function restart() {
    if (inFlight || stopping || !current || current.exited) return;
    inFlight = true;
    const old = current;
    try {
      log('tower: source changed — restarting…');
      send(old, 'release');
      const released = await waitFor(old, 'released', releaseTimeoutMs);
      if (!released && !old.exited) {
        log('tower: the running server did not release its port — stopping it.');
        await stopWorker(old);
      }
      // A browser opened once at first boot; restarts never open another.
      const next = start(argv.filter((a) => a !== '--open'));
      if (await waitFor(next, 'listening', readyTimeoutMs)) {
        current = next;
        await stopWorker(old);
        log('tower: restarted on the new source.');
        return;
      }
      await stopWorker(next);
      if (old.exited) {
        log(`tower: the restarted server failed (code ${next.code}) and the previous one is gone — exiting.`);
        exitFn(next.code || 1);
        return;
      }
      log(`tower: the restarted server failed (code ${next.code}) — the previous server keeps running; the stale-server banner shows until the source is fixed and \`tower serve\` is restarted.`);
      send(old, 'reopen');
    } finally {
      inFlight = false;
    }
  }

  function scheduleRestart() {
    if (inFlight || stopping) return;
    clearTimeout(timer);
    timer = setTimeout(restart, debounceMs);
  }

  current = start(argv);

  const dirs = [towerRoot, join(towerRoot, 'app')];
  const watchers = dirs.map((d) => watch(d, { persistent: true }, (_event, filename) => {
    if (filename && filename.endsWith('.mjs')) scheduleRestart();
  }));

  function stop() {
    clearTimeout(timer);
    watchers.forEach((w) => w.close());
  }

  // Ctrl-C / service stop: forward to the worker and exit with it.
  if (handleSignals) {
    for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
      process.on(signal, () => {
        stopping = true;
        stop();
        if (current.exited) return exitFn(current.code ?? 0);
        current.child.kill(signal);
      });
    }
  }

  // `restartNow` bypasses the fs.watch debounce for direct callers/tests;
  // `stop` tears down the watchers (tests must call this to let node exit).
  return { stop, restartNow: restart, worker: () => current };
}

// Worker side of `tower serve --worker`: serves, reports `listening`,
// releases or reopens its port on request, and exits when the supervisor's
// IPC channel goes away so a dead supervisor never leaves an orphan server.
// serveFn(open) starts one HTTP server (server.mjs `serve`).
export function serveWorker({
  serveFn,
  open = false,
  channel = process,
  exitFn = (code) => process.exit(code),
}) {
  let server = null;
  const listen = (openBrowser) => {
    server = serveFn(openBrowser);
    server.once('listening', () => channel.send?.({ tower: 'listening' }));
  };
  listen(open);
  channel.on('message', (m) => {
    if (m?.tower === 'release') {
      server.close(() => channel.send?.({ tower: 'released' }));
      // SSE clients hold keep-alive sockets open forever; close() alone
      // would wait on them indefinitely.
      server.closeAllConnections?.();
    } else if (m?.tower === 'reopen') {
      listen(false);
    }
  });
  channel.on('disconnect', () => exitFn(0));
  return () => server;
}
