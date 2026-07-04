// Cross-process write lock: a lock DIRECTORY next to the data file (mkdir is
// atomic on every platform). Holds pid + timestamp; stale locks (dead pid or
// too old) are broken. Serializes CLI vs server vs concurrent agents.
import { mkdirSync, rmSync, readFileSync, writeFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';

const STALE_MS = 15_000;

function pidAlive(pid) {
  try { process.kill(pid, 0); return true; } catch (e) { return e.code === 'EPERM'; }
}

export function withLock(file, fn) {
  const dir = `${file}.lock`;
  const info = join(dir, 'info.json');
  const deadline = Date.now() + 10_000;
  for (;;) {
    try {
      mkdirSync(dir);
      writeFileSync(info, JSON.stringify({ pid: process.pid, at: Date.now() }));
      break;
    } catch (e) {
      if (e.code !== 'EEXIST') throw e;
      let stale = true;
      try {
        const held = JSON.parse(readFileSync(info, 'utf8'));
        stale = Date.now() - held.at > STALE_MS || !pidAlive(held.pid);
      } catch { /* unreadable info → likely mid-create; only stale if dir is old */ }
      if (stale) { try { rmSync(dir, { recursive: true, force: true }); } catch { /* raced */ } continue; }
      if (Date.now() > deadline) throw new Error(`tower: could not acquire write lock at ${dir} (held by another process)`);
      const until = Date.now() + 50;
      while (Date.now() < until) { /* brief spin; sync context */ }
    }
  }
  try { return fn(); }
  finally { try { rmSync(dir, { recursive: true, force: true }); } catch { /* already gone */ } }
}
