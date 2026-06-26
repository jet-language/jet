// Shared paths + tiny utilities. Std-only, zero deps.
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { readFileSync, writeFileSync, existsSync } from 'node:fs';

const here = dirname(fileURLToPath(import.meta.url));
export const ROOT = dirname(here);                 // tools/Tower-v2
export const REPO = dirname(dirname(ROOT));         // repo root
export const UI = join(here, 'ui');
export const DATA = join(ROOT, 'tower.json');
export const V1 = join(dirname(ROOT), 'Tower');     // tools/Tower (read-only source for migrate)

export const readJSON = (p, fallback) =>
  existsSync(p) ? JSON.parse(readFileSync(p, 'utf8')) : fallback;
export const writeJSON = (p, v) => writeFileSync(p, JSON.stringify(v, null, 2) + '\n');
export const readText = (p) => (existsSync(p) ? readFileSync(p, 'utf8') : '');

// A monotonic-ish counter avoids Date.now()/Math.random() (kept deterministic-friendly).
let seq = 0;
export const newId = (prefix) => `${prefix}${(seq++).toString(36)}${process.hrtime.bigint().toString(36).slice(-5)}`;
export const today = () => new Date().toISOString().slice(0, 10);
export const now = () => new Date().toISOString();
