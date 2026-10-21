#!/usr/bin/env node
// census.mjs --jet SPIKE --root TREE --out FILE.tsv [--all] [--table TSV] [--jobs N] [--batch N] [FILTER...]
// IR builder coverage over the corpus: `jet ir census` (no x64) on every
// Examples/features program the old backend passes (PASS rows of --table), in
// --jobs parallel processes of --batch files each, run from TREE (an export,
// never the live checkout). Writes every failure row to FILE.tsv and prints
// the histogram of failure messages over all rows and over each program's
// first failure. --all builds every checked function, not only the entry's reach.
import { spawn } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const usage = 'census.mjs --jet SPIKE --root TREE --out FILE.tsv [--all] [--table TSV] [--jobs N] [--batch N] [FILTER...]';
const options = { table: '/mnt/jetscratch/discovery/golden/full-m-7/table.tsv', jobs: 8, batch: 8, all: false, filters: [] };
for (let i = 2; i < process.argv.length; i++) {
  const flag = process.argv[i];
  if (flag === '--all') { options.all = true; continue; }
  const key = { '--jet': 'jet', '--root': 'root', '--out': 'out', '--table': 'table', '--jobs': 'jobs', '--batch': 'batch' }[flag];
  if (!key) { if (flag.startsWith('--')) throw Error(usage); options.filters.push(flag); continue; }
  const value = process.argv[++i];
  if (value === undefined) throw Error(usage);
  options[key] = key === 'jobs' || key === 'batch' ? Number(value) : resolve(value);
}
if (!options.jet || !options.root || !options.out) throw Error(usage);
const files = readFileSync(options.table, 'utf8').split('\n').map((l) => l.split('\t'))
  .filter((r) => r[1] === 'PASS' && r[0].startsWith('Examples/features/')).map((r) => r[0])
  .filter((f) => !options.filters.length || options.filters.some((n) => f.includes(n)));
const batches = [];
for (let i = 0; i < files.length; i += options.batch) batches.push(files.slice(i, i + options.batch));

function census(batch) {
  return new Promise((done) => {
    const child = spawn(options.jet, ['ir', 'census', ...(options.all ? ['--all'] : []), ...batch], {
      cwd: options.root, env: { ...process.env, NO_COLOR: '1', JET_TOOLCHAIN_ROOT: options.root }, stdio: ['ignore', 'pipe', 'pipe'],
    });
    let out = '', err = '';
    child.stdout.on('data', (c) => { out += c; });
    child.stderr.on('data', (c) => { err += c; });
    child.on('close', (code, signal) => {
      const rows = out.split('\n').filter((l) => l && !l.startsWith('#'));
      const seen = new Set(rows.map((r) => r.split('\t')[0]));
      // A process that died mid-batch leaves its unreported files as crash rows.
      for (const file of batch) if (!seen.has(file)) rows.push(`${file}\t-\t-\tcensus crashed (exit ${code ?? signal}): ${err.split('\n').find((l) => l.trim()) ?? ''}\t-`);
      done(rows);
    });
  });
}

const rows = [];
let next = 0;
await Promise.all(Array.from({ length: Math.min(options.jobs, batches.length) }, async () => {
  while (next < batches.length) rows.push(...await census(batches[next++]));
}));
rows.sort();
writeFileSync(options.out, 'file\tkey\tname\tmessage\tlocation\n' + rows.join('\n') + '\n');
const group = (m) => m.replace(/\d+/g, 'N').slice(0, 160);
const cells = rows.map((r) => r.split('\t'));
// Per-file summary rows (`ok N functions` / `built N functions, failed M`) are the denominator, not failures.
const summary = /^(ok \d+ functions|built \d+ functions, failed \d+)$/;
const failures = cells.filter((r) => !(r[1] === '-' && summary.test(r[3])));
const builtFunctions = cells.filter((r) => r[1] === '-' && summary.test(r[3])).reduce((sum, r) => sum + Number(r[3].match(/\d+/)[0]), 0);
const histogram = (list) => { const h = new Map(); for (const g of list) h.set(g, (h.get(g) ?? 0) + 1); return [...h].sort((a, b) => b[1] - a[1]); };
const firsts = new Map();
for (const r of failures) if (!firsts.has(r[0])) firsts.set(r[0], r[3]);
const lines = [`programs ${files.length}  built clean ${files.length - firsts.size}  with failures ${firsts.size}  functions built ${builtFunctions}  failure rows ${failures.length}`, '', '# by first failure (programs)'];
for (const [g, n] of histogram([...firsts.values()].map(group))) lines.push(`${String(n).padStart(5)}  ${g}`);
lines.push('', '# every failure row (functions)');
for (const [g, n] of histogram(failures.map((r) => group(r[3])))) lines.push(`${String(n).padStart(5)}  ${g}`);
writeFileSync(options.out.replace(/\.tsv$/, '') + '.summary.txt', lines.join('\n') + '\n');
process.stdout.write(lines.join('\n') + '\n');
