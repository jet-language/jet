// Self-restart on source change. `tower serve` loads all its route/db code
// once at process start (see server.mjs) — if an agent edits that code and
// never restarts the process, the served UI (read fresh off disk every
// request) looks current while the running process is still the old one,
// so new endpoints 404 forever. This watches Tower's own source and swaps
// the process out from under itself when it changes.
//
// Design: on a source change, gracefully release the port, spawn a fresh
// copy of this exact command (same argv — an exec-style self-replace, minus
// an actual execve syscall Node doesn't expose), and watch whether it
// survives a short crash window. If it does, this process hands off and
// exits. If the new process dies fast (syntax error, throw-on-boot, ...),
// this process reopens its own still-working server instead of retrying —
// no restart-loop — and the version-mismatch banner (see version.mjs +
// server.mjs's /api/version) covers telling the owner a manual fix and
// restart are needed.
import { watch } from 'node:fs';
import { spawn as defaultSpawn } from 'node:child_process';
import { join } from 'node:path';

const DEBOUNCE_MS = 300;
const CRASH_WINDOW_MS = 2000;

// towerRoot: dir containing tower.mjs. argv: process.argv.slice(2) (the
// original CLI args, so the respawned process runs the identical command).
// getServer/reopen: how to release / re-acquire this process's own HTTP
// server around a restart attempt.
export function watchForRestart({
  towerRoot,
  argv,
  getServer,
  reopen,
  log = (...a) => console.log(...a),
  spawnFn = defaultSpawn,
  exitFn = (code) => process.exit(code),
  debounceMs = DEBOUNCE_MS,
  crashWindowMs = CRASH_WINDOW_MS,
}) {
  let timer = null;
  let inFlight = false;

  function scheduleRestart() {
    if (inFlight) return;
    clearTimeout(timer);
    timer = setTimeout(doRestart, debounceMs);
  }

  async function doRestart() {
    inFlight = true;
    try {
      log('tower: source changed — restarting…');
      const server = getServer();
      await new Promise((resolve) => {
        if (!server) return resolve();
        server.close(() => resolve());
        // SSE clients hold long-lived keep-alive sockets open forever —
        // server.close() alone would wait on them indefinitely. Give
        // in-flight requests a brief grace window, then force the rest shut
        // so the restart can proceed.
        setTimeout(() => { try { server.closeAllConnections?.(); } catch { /* already closed */ } }, 200);
      });

      const spawnedAt = Date.now();
      const child = spawnFn(process.execPath, [process.argv[1], ...argv], { stdio: 'inherit', detached: true });
      let settled = false;

      await new Promise((resolve) => {
        child.on('exit', (code) => {
          if (settled) return;
          settled = true;
          if (Date.now() - spawnedAt < crashWindowMs) {
            log(`tower: restarted process exited immediately (code ${code}) — staying on this process; the stale-server banner will show until the source is fixed and \`tower serve\` is restarted.`);
            reopen();
          }
          resolve();
        });
        setTimeout(() => {
          if (settled) return;
          settled = true;
          child.unref();
          log('tower: new process is up — handing off.');
          exitFn(0);
          resolve();
        }, crashWindowMs);
      });
    } finally {
      inFlight = false;
    }
  }

  const dirs = [towerRoot, join(towerRoot, 'app')];
  const watchers = dirs.map((d) => watch(d, { persistent: true }, (_event, filename) => {
    if (filename && filename.endsWith('.mjs')) scheduleRestart();
  }));
  // `restartNow` bypasses the fs.watch debounce for direct callers/tests;
  // `stop` tears down the watchers (tests must call this to let node exit).
  return { stop: () => watchers.forEach((w) => w.close()), restartNow: doRestart };
}
