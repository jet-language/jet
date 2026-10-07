#!/usr/bin/env node
// Same-tree parity receipt: 50 golden and 50 UI cases, Rust libtest versus the
// Cargo-free expectation runner. Build/admission is deliberately the caller's.
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
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
for (const key of ['jet', 'golden', 'ui']) if (!options[key]) throw new Error(`--${key} is required`);
const run = (binary, args, env, timeout = options.timeout) => {
  const result = spawnSync(binary, args, { cwd: ROOT, env, detached: true, encoding: 'utf8', timeout: timeout * 1000, killSignal: 'SIGKILL', maxBuffer: 64 * 1024 * 1024 });
  if (result.error || result.signal) { try { process.kill(-result.pid, 'SIGKILL'); } catch {} }
  return result;
};
// test-corpora bounds each child; the outer deadline allows every case to run.
const nodeResult = run(process.execPath, [join(ROOT, 'Tools/agent/test-corpora.mjs'), '--jet', resolve(options.jet), '--suite', 'all', '--cases', list, '--rust-audit', '--timeout', String(options.timeout), '--results', join(options.out, 'node.json')], { ...process.env, JET_TEST_SCRATCH_DIR: options.out }, options.timeout * cases.length * 3);
if (nodeResult.error) throw nodeResult.error;
writeFileSync(join(options.out, 'node.log'), nodeResult.stdout + nodeResult.stderr);
const nodeRows = JSON.parse(readFileSync(join(options.out, 'node.json'), 'utf8')).results;
const rows = [];
for (const [index, entry] of cases.entries()) {
  const filter = entry.suite === 'golden' ? 'JET_GOLDEN_FILTER' : 'JET_UI_FILTER';
  const env = { ...process.env, [filter]: entry.shown, JET_TEST_JOBS: '1', NO_COLOR: '1', JET_TEST_SCRATCH_DIR: options.out };
  delete env.UPDATE_EXPECT; delete env.JET_UPDATE_GOLDEN;
  const reference = run(resolve(options[entry.suite]), [entry.suite === 'golden' ? 'examples_compile_and_run' : 'ui_snapshots', '--exact', '--nocapture', '--test-threads=1'], env);
  const log = join(options.out, `rust-${index}.log`); writeFileSync(log, (reference.stdout ?? '') + (reference.stderr ?? ''));
  const rust = reference.error || reference.signal ? 'UNAVAILABLE' : reference.status === 0 ? 'PASS' : 'FAIL';
  const candidate = nodeRows.find((row) => row.case === entry.shown);
  const node = candidate?.status ?? 'UNAVAILABLE';
  const agreement = ['PASS', 'FAIL'].includes(rust) && rust === node;
  rows.push({ suite: entry.suite, case: entry.shown, rust, node, agreement, rustLog: log, nodeDetail: candidate?.detail ?? '' });
  console.log(`${agreement ? 'AGREE' : 'DIFFER'}\t${entry.shown}\t${rust}\t${node}`);
}
const receipt = { jet: resolve(options.jet), golden: resolve(options.golden), ui: resolve(options.ui), cases: rows.length, agreeing: rows.filter((row) => row.agreement).length, differing: rows.filter((row) => !row.agreement).length, rows };
writeFileSync(join(options.out, 'parity.json'), JSON.stringify(receipt, null, 2) + '\n');
writeFileSync(join(options.out, 'parity.tsv'), 'suite\tcase\trust\tnode\tagreement\n' + rows.map((row) => [row.suite, row.case, row.rust, row.node, row.agreement ? 'AGREE' : 'DIFFER'].join('\t')).join('\n') + '\n');
console.log(`${receipt.agreeing}/${receipt.cases} verdicts agree; ${receipt.differing} differ`);
process.exitCode = receipt.differing ? 1 : 0;
