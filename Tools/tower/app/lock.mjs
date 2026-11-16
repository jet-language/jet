// Cross-process write lock: a lock DIRECTORY next to the data file (mkdir is
// atomic on every platform). Holds pid + timestamp; stale locks (dead pid or
// too old) are broken. Serializes CLI vs server vs concurrent agents.
import { mkdirSync, rmSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const STALE_MS = 15_000;

function pidAlive(pid) {
  try { process.kill(pid, 0); return true; } catch (e) { return e.code === 'EPERM'; }
}

// One acquisition attempt: true when this process now holds the lock. A
// stale lock (dead owner, or older than STALE_MS) is broken and retried.
function tryAcquire(dir, info) {
  for (;;) {
    try {
      mkdirSync(dir);
      writeFileSync(info, JSON.stringify({ pid: process.pid, at: Date.now() }));
      return true;
    } catch (e) {
      if (e.code !== 'EEXIST') throw e;
      let stale = true;
      try {
        const held = JSON.parse(readFileSync(info, 'utf8'));
        stale = Date.now() - held.at > STALE_MS || !pidAlive(held.pid);
      } catch {
        // The winner creates the directory before it can write info.json.
        // Treat that ordinary publication window as live; only an old lock
        // directory with no readable owner record is safe to break.
        try { stale = Date.now() - statSync(dir).mtimeMs > STALE_MS; }
        catch { stale = false; }
      }
      if (!stale) return false;
      try { rmSync(dir, { recursive: true, force: true }); } catch { /* raced */ }
    }
  }
}

function release(dir) {
  try { rmSync(dir, { recursive: true, force: true }); } catch { /* already gone */ }
}

const lockFailure = (dir) => new Error(`tower: could not acquire write lock at ${dir} (held by another process)`);

export function withLock(file, fn) {
  const dir = `${file}.lock`;
  const info = join(dir, 'info.json');
  const deadline = Date.now() + 10_000;
  while (!tryAcquire(dir, info)) {
    if (Date.now() > deadline) throw lockFailure(dir);
    const until = Date.now() + 50;
    while (Date.now() < until) { /* brief spin; sync context */ }
  }
  try { return fn(); }
  finally { release(dir); }
}

// The server's variant: wait for the lock without blocking the event loop,
// so reads and the live stream keep flowing while agents hold it, and wait
// long enough that a busy board never rejects an owner write.
export async function withLockAsync(file, fn, { timeoutMs = 120_000, pollMs = 15 } = {}) {
  const dir = `${file}.lock`;
  const info = join(dir, 'info.json');
  const deadline = Date.now() + timeoutMs;
  while (!tryAcquire(dir, info)) {
    if (Date.now() > deadline) throw lockFailure(dir);
    await new Promise(resolve => setTimeout(resolve, pollMs));
  }
  try { return fn(); }
  finally { release(dir); }
}
