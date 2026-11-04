// era.mjs: split the REOPEN rows of CLOSURE-AUDIT-v2.tsv by when they were closed,
// relative to the Rust cutover (R7a2 6a18f4d8e3, 2026-10-23T07:18:29Z).
// Usage: node era.mjs [DIR]. Reads DIR/CLOSURE-AUDIT-v2.tsv and writes DIR/REOPEN-{live,postcut,precut}.tsv
// (default DIR: beside this script); tally on stdout. Tower is read as audit2.mjs reads it.
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';

const HERE = path.dirname(new URL(import.meta.url).pathname);
const DIR = process.argv[2] || HERE;
const env = { ...process.env }; delete env.GIT_INDEX_FILE;
const REPO = path.dirname(execFileSync('git', ['rev-parse', '--path-format=absolute', '--git-common-dir'], { cwd: HERE, env, encoding: 'utf8' }).trim());
const TOWER = process.env.TOWER_DIR || `${REPO}/Tools/tower/.tower`;
const CUT = Date.parse('2026-10-23T07:18:29Z');
const live = JSON.parse(fs.readFileSync(`${TOWER}/tower.json`, 'utf8'));
const hist = JSON.parse(fs.readFileSync(`${TOWER}/history.json`, 'utf8'));
const byNum = new Map();
for (const c of [...(hist.cards || []), ...live.cards]) byNum.set(c.num, c);

const lines = fs.readFileSync(path.join(DIR, 'CLOSURE-AUDIT-v2.tsv'), 'utf8').split('\n');
const head = lines[1];
const buckets = { live: [], postcut: [], precut: [] };
const tally = {};
for (const line of lines.slice(2)) {
  if (!line) continue;
  const f = line.split('\t');
  if (f[5] !== 'REOPEN') continue;
  const m = f[1].match(/^#(\d+)(?: c(\d+))?/);
  const c = byNum.get(+m[1]);
  const k = m[2] ? (c?.criteria || []).find((x) => x.n === +m[2]) : null;
  // Post-cutover when the card closed or the criterion was met after the cutover
  // (the conservative union: either date counts). retiredAt is when it was archived, not closed.
  const dates = [c?.completedAt, k && k.at].filter(Boolean);
  if (!dates.length && c) dates.push(c.updated || c.created || '');
  const times = dates.map((d) => Date.parse(d)).filter((t) => !Number.isNaN(t));
  const when = dates.join('|');
  const era = !times.length ? 'unknown' : times.some((t) => t >= CUT) ? 'post' : 'pre';
  const merged = /merged into #/.test(f[7]) ? 'merged' : 'other';
  const b = f[2] === 'live' ? 'live' : era === 'post' ? 'postcut' : 'precut';
  buckets[b].push([when, ...f].join('\t'));
  const key = `${f[2]}:${era}:${merged}`;
  tally[key] = (tally[key] || 0) + 1;
}
const cardsOf = (rows) => new Set(rows.map((r) => r.split('\t')[2].split(' ')[0])).size;
for (const [b, rows] of Object.entries(buckets)) fs.writeFileSync(path.join(DIR, `REOPEN-${b}.tsv`), `closed_at\t${head}\n${rows.sort().join('\n')}\n`);
console.log(JSON.stringify({ tally, rows: Object.fromEntries(Object.entries(buckets).map(([b, r]) => [b, r.length])), cards: Object.fromEntries(Object.entries(buckets).map(([b, r]) => [b, cardsOf(r)])) }, null, 1));
