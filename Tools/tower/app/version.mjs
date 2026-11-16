// Content-hash freshness marker for Tower's own source — independent of the
// self-restart watcher (restart.mjs). Even if the watcher fails to swap the
// process, this lets a freshly-served page (always read straight off disk,
// see server.mjs's serveStatic) notice its own source is newer than what the
// running process loaded at boot, and show a stale-server banner.
//
// Only the modules imported once at process start can go stale: tower.mjs
// (entry) and every app/*.mjs (routes, db, etc). Files under app/ui
// are re-read from disk on every request, so they never need this.
import { createHash } from 'node:crypto';
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';

export function sourceFiles(towerRoot) {
  const appDir = join(towerRoot, 'app');
  const files = [
    join(towerRoot, 'tower.mjs'),
    ...readdirSync(appDir).filter((f) => f.endsWith('.mjs')).map((f) => join(appDir, f)),
  ];
  return files.sort();
}

export function computeVersion(towerRoot) {
  const hash = createHash('sha256');
  for (const f of sourceFiles(towerRoot)) {
    hash.update(f);
    try { hash.update(readFileSync(f)); } catch { /* deleted mid-scan — ignore, hash still deterministic enough */ }
  }
  return hash.digest('hex').slice(0, 12);
}
