// Paths + tiny utilities. Std-only, zero deps.
//
// Tower the tool lives wherever it's installed (plugin dir, cloned repo,
// vendored copy). Tower the DATA lives in the host project, discovered at
// runtime:
//   1. TOWER_DATA env var — explicit path to a tower.json
//   2. nearest `.tower/tower.json` walking up from cwd
//   3. nowhere → commands that need data fail with a "run `tower init`" hint
import { fileURLToPath } from 'node:url';
import { dirname, join, resolve } from 'node:path';
import { readFileSync, writeFileSync, renameSync, mkdirSync, existsSync, copyFileSync, readdirSync, unlinkSync } from 'node:fs';

const here = dirname(fileURLToPath(import.meta.url));
export const TOOL_ROOT = dirname(here);            // plugin root
export const UI = join(here, 'ui');

export function findDataDir(from = process.cwd()) {
  if (process.env.TOWER_DATA) {
    const p = resolve(process.env.TOWER_DATA);
    return p.endsWith('.json') ? dirname(p) : p;
  }
  let dir = resolve(from);
  for (;;) {
    if (existsSync(join(dir, '.tower', 'tower.json'))) return join(dir, '.tower');
    const parent = dirname(dir);
    if (parent === dir) return null;
    dir = parent;
  }
}

export function dataFile(dir = findDataDir()) {
  if (!dir) return null;
  if (process.env.TOWER_DATA && process.env.TOWER_DATA.endsWith('.json')) return resolve(process.env.TOWER_DATA);
  return join(dir, 'tower.json');
}
export const configFile = (dir) => (dir ? join(dir, 'config.json') : null);
export const secretsFile = (dir) => (dir ? join(dir, 'secrets.json') : null);
// Append-only archive (#461): retired cards/decisions/events, same dir as
// tower.json, committed to git (NOT gitignored — it's board history).
export const historyFile = (dir) => (dir ? join(dir, 'history.json') : null);

export const readJSON = (p, fallback) =>
  p && existsSync(p) ? JSON.parse(readFileSync(p, 'utf8')) : fallback;

// Atomic write: tmp file + rename, so a crash mid-write never corrupts data.
export function writeJSON(p, v) {
  const tmp = `${p}.tmp.${process.pid}`;
  writeFileSync(tmp, JSON.stringify(v, null, 2) + '\n');
  renameSync(tmp, p);
}

// Rolling backups: keep the last N copies in <dataDir>/backups/.
export function backup(p, keep = 20) {
  try {
    if (!existsSync(p)) return;
    const dir = join(dirname(p), 'backups');
    mkdirSync(dir, { recursive: true });
    const stamp = new Date().toISOString().replace(/[:.]/g, '-');
    copyFileSync(p, join(dir, `tower-${stamp}.json`));
    const old = readdirSync(dir).filter(f => f.startsWith('tower-')).sort();
    for (const f of old.slice(0, Math.max(0, old.length - keep))) unlinkSync(join(dir, f));
  } catch { /* backups are best-effort; never block a write */ }
}

let seq = 0;
export const newId = (prefix) => `${prefix}${(seq++).toString(36)}${process.hrtime.bigint().toString(36).slice(-6)}`;
export const today = () => new Date().toISOString().slice(0, 10);
export const now = () => new Date().toISOString();
