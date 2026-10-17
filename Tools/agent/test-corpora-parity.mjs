#!/usr/bin/env node
// Native parity sampler: 50 golden and 50 UI cases through the Cargo-free
// expectation runner. The Rust libtest oracle is retired; live comparison is UNCOVERED.
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { collectGoldenEntries, loadExampleStdin } from './compiler-diff.mjs';
import { uiEntries, uiInvocation } from './test-corpora.mjs';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const EXPECTED = join(ROOT, 'Examples/features/expected');
const options = { timeout: 600 };
for (let i = 2; i < process.argv.length; i++) {
  if (process.argv[i] === '--sample') { options.sample = true; continue; }
  const key = ({ '--jet': 'jet', '--golden': 'golden', '--ui': 'ui', '--out': 'out', '--cases': 'cases', '--timeout': 'timeout' })[process.argv[i]];
  if (!key || !process.argv[i + 1]) throw new Error(`unknown or incomplete option ${process.argv[i]}`);
  options[key] = process.argv[++i];
}
if (!options.out) throw new Error('usage: test-corpora-parity.mjs --out DIR [--sample | --jet JET --golden GOLDEN_LIBTEST --ui UI_LIBTEST] [--cases FILE]');
options.out = resolve(options.out); mkdirSync(options.out, { recursive: true });
options.timeout = Number(options.timeout);
if (!Number.isFinite(options.timeout) || options.timeout <= 0) throw new Error('timeout must be positive');
const hash = (id) => createHash('sha256').update(id).digest('hex');
const byHash = (a, b) => hash(a.shown).localeCompare(hash(b.shown), 'en');
const stdin = loadExampleStdin();
const golden = collectGoldenEntries().map((entry) => ({ ...entry, suite: 'golden' }));
const ui = uiEntries();
const all = [...golden, ...ui];
let cases;
if (options.cases) {
  const ids = readFileSync(options.cases, 'utf8').split('\n').filter((line) => line && !line.startsWith('#'));
  if (new Set(ids).size !== ids.length) throw new Error('duplicate parity cases');
  cases = ids.map((id) => { const entry = all.find((entry) => entry.shown === id); if (!entry) throw new Error(`unknown parity case ${id}`); return entry; });
} else {
  // Sample only the explicitly implemented fixture mechanisms, never by their
  // observed PASS/FAIL result. Unsupported mechanisms remain REFUSED in full
  // runs and are not represented as covered by this receipt.
  const supportedGolden = golden.filter((entry) => {
    const source = readFileSync(entry.path, 'utf8');
    return entry.stem !== 'devloop/job_runner' && !entry.stem.startsWith('lowlevel/polyglot_') && !/#Unsafe\b/u.test(source) && (existsSync(join(EXPECTED, `${entry.stem}.out`)) || existsSync(join(EXPECTED, `${entry.stem}.err.out`)));
  }).sort(byHash);
  const chosen = [];
  const add = (entry) => { if (entry && !chosen.includes(entry)) chosen.push(entry); };
  // Pin representatives of each stream/staging rule in addition to hash sampling.
  for (const select of [
    (entry) => existsSync(join(EXPECTED, `${entry.stem}.err.out`)),
    (entry) => existsSync(join(EXPECTED, `${entry.stem}.stderr.out`)),
    (entry) => stdin.has(entry.stem),
    (entry) => basename(entry.path) === 'run.jet',
    (entry) => entry.stem.startsWith('cli/'),
  ]) add(supportedGolden.find(select));
  for (const entry of supportedGolden) { if (chosen.length === 50) break; add(entry); }
  const supportedUi = ui.filter((entry) => !uiInvocation(readFileSync(entry.path, 'utf8'), entry.shown).refused).sort(byHash);
  cases = [...chosen, ...supportedUi.slice(0, 50)];
}
for (const suite of ['golden', 'ui']) if (cases.filter((entry) => entry.suite === suite).length !== 50) throw new Error(`parity requires exactly 50 ${suite} cases`);
const list = join(options.out, 'cases.txt');
writeFileSync(list, cases.map((entry) => entry.shown).join('\n') + '\n');
if (options.sample) { console.log(`sample: 50 golden + 50 UI; ${list}`); process.exit(0); }
// Rust libtest is the retired oracle; this native runner never invokes Cargo or the Rust host.
console.log(JSON.stringify({ status: 'UNCOVERED', cardRequired: true, reason: 'Rust libtest parity oracle is retired; no Cargo or Rust host invocation' }));
process.exitCode = 2;
