#!/usr/bin/env node
// differential.mjs --jet SPIKE --root TREE --out FRESH_DIR [--table TSV] [--jobs N] [--timeout S] [FILTER...]
// The slot IR route against the ordinary backend, both from the SAME spike
// binary: per Examples/features program (default: the PASS rows of --table),
// `jet build` and `jet ir build` (both traced), then both executables, each in
// its own staged copy of the program (Tools/agent/run-feature-examples.mjs
// staging and environment; stdin closed). Compared byte for byte: build exit
// code (and stderr when a build fails), run stdout, stderr and exit code. The
// IR side is also checked against Examples/features/expected (classifyRun).
// Writes FRESH_DIR/differential.tsv, FRESH_DIR/summary.txt (identical /
// different / failed counts, IR failures grouped by first message, per-phase
// sums old vs IR, old-fail/IR-pass rows against the golden) and the rows'
// stdout/stderr under FRESH_DIR/cases/. Exit 0 when every program is identical.
import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, relative, resolve } from 'node:path';
import { classifyRun, collectFeatureExamples, copyFeatureProject, featureProjectRoot } from '../../../Tools/agent/run-feature-examples.mjs';

const usage = 'differential.mjs --jet SPIKE --root TREE --out FRESH_DIR [--table TSV] [--jobs N] [--timeout S] [FILTER...]';
const options = { table: '/mnt/jetscratch/discovery/golden/full-m-7/table.tsv', jobs: 8, timeout: 120, filters: [] };
for (let i = 2; i < process.argv.length; i++) {
  const flag = process.argv[i];
  const key = { '--jet': 'jet', '--root': 'root', '--out': 'out', '--table': 'table', '--jobs': 'jobs', '--timeout': 'timeout' }[flag];
  if (!key) { if (flag.startsWith('--')) throw Error(usage); options.filters.push(flag); continue; }
  const value = process.argv[++i];
  if (value === undefined) throw Error(usage);
  options[key] = key === 'jobs' || key === 'timeout' ? Number(value) : resolve(value);
}
if (!options.jet || !options.root || !options.out) throw Error(usage);
if (existsSync(options.out)) throw Error(`output already exists; use a fresh directory: ${options.out}`);
const ROOT = options.root;
const FEATURES = join(ROOT, 'Examples/features');
const EXPECTED = join(FEATURES, 'expected');
mkdirSync(join(options.out, 'cases'), { recursive: true });

// The programs the old backend passes, from the golden table (Examples/features PASS rows).
const passing = new Set(readFileSync(options.table, 'utf8').split('\n').map((line) => line.split('\t'))
  .filter((row) => row[1] === 'PASS' && row[0].startsWith('Examples/features/')).map((row) => row[0]));
const entries = collectFeatureExamples(FEATURES)
  .filter((entry) => passing.has(entry.shown))
  .filter((entry) => !options.filters.length || options.filters.some((needle) => entry.shown.includes(needle)));

function readExpected(stem) {
  const read = (name) => { const path = join(EXPECTED, name); return existsSync(path) ? readFileSync(path, 'utf8') : null; };
  return { out: read(`${stem}.out`), err: read(`${stem}.err.out`), stderrOut: read(`${stem}.stderr.out`) };
}

// A staged copy of the entry under `stage`, at its display path: its project
// root, else the directory holding it (sibling sources and fixtures).
function stage(entry, stageRoot) {
  const source = featureProjectRoot(entry) ?? dirname(entry.path);
  copyFeatureProject(source, join(stageRoot, relative(ROOT, source)));
  if (!existsSync(join(stageRoot, entry.shown))) throw Error(`staging lost ${entry.shown}`);
}

function run(command, args, cwd, scratch, slot) {
  return new Promise((done) => {
    const started = process.hrtime.bigint();
    const child = spawn(command, args, {
      cwd,
      env: { ...process.env, NO_COLOR: '1', CLICOLOR: '0', JET_STORE_DIR: join(scratch, 'store', String(slot)), TMPDIR: scratch, TMP: scratch, TEMP: scratch, JET_TOOLCHAIN_ROOT: ROOT },
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    const out = [], err = [];
    let timedOut = false;
    child.stdout.on('data', (chunk) => out.push(chunk));
    child.stderr.on('data', (chunk) => err.push(chunk));
    const timer = setTimeout(() => { timedOut = true; child.kill('SIGKILL'); }, options.timeout * 1000);
    const finish = (code, signal, extra = '') => {
      clearTimeout(timer);
      done({ code, signal, timedOut, timeoutSec: options.timeout, stdout: Buffer.concat(out), stderr: Buffer.concat([...err, Buffer.from(extra)]), ms: Number(process.hrtime.bigint() - started) / 1e6 });
    };
    child.on('close', (code, signal) => finish(code, signal));
    child.on('error', (error) => finish(null, null, `${error.message}\n`));
  });
}

// Wall ms of each top-level span of a Chrome trace file, summed per name
// (/mnt/jetscratch/lanes/IRArch/tools/phases.mjs).
function phases(file) {
  const totals = {};
  if (!existsSync(file)) return totals;
  const events = readFileSync(file, 'utf8').split('\n').map((l) => l.trim().replace(/,$/, ''))
    .filter((l) => l.startsWith('{')).map((l) => { try { return JSON.parse(l); } catch { return null; } }).filter(Boolean);
  const stacks = {};
  for (const e of events) {
    if (e.ph !== 'B' && e.ph !== 'E') continue;
    const s = stacks[e.tid] ??= [];
    if (e.ph === 'B') { s.push(e); continue; }
    const b = s.pop();
    if (b && s.length === 0) totals[b.name] = (totals[b.name] ?? 0) + (e.ts - b.ts) / 1000;
  }
  return totals;
}

function findBuilt(dir, name) {
  let found = null;
  const walk = (d) => {
    for (const item of readdirSync(d, { withFileTypes: true })) {
      const path = join(d, item.name);
      if (item.isDirectory()) { if (item.name === '.jet' && existsSync(join(path, 'build', name))) found ??= join(path, 'build', name); walk(path); }
    }
  };
  walk(dir);
  return found;
}

const text = (buffer) => buffer.toString('utf8');
const firstLine = (s) => s.split('\n').find((l) => l.trim() !== '')?.trim() ?? '';
// The IR route's failure rows (`  ir.<phase> ...` under its error line).
const irFailures = (stderr) => stderr.split('\n').filter((l) => /^ {2}ir\.(build|x64|object|link) /.test(l)).map((l) => l.trim());
// A failure row's group: its phase and message, without the function, span and numbers.
const failureGroup = (row) => row.replace(/^(ir\.build) .*? \[[^\]]*\]: /, '$1 ').replace(/ at [^ ]+:\d+\.\.\d+$/, '').replace(/\d+/g, 'N').slice(0, 160);

async function compare(entry, slot) {
  const scratch = join(options.out, 'scratch', String(slot));
  mkdirSync(scratch, { recursive: true });
  const caseDir = join(options.out, 'cases', entry.stem.replaceAll('/', '__'));
  mkdirSync(caseDir, { recursive: true });
  const expected = readExpected(entry.stem);
  const side = async (kind) => {
    const root = join(scratch, kind);
    rmSync(root, { recursive: true, force: true });
    stage(entry, root);
    const trace = join(caseDir, `${kind}.trace.json`);
    const exe = join(root, `${basename(entry.stem)}.ir.exe`);
    const build = kind === 'old'
      ? await run(options.jet, ['build', `--trace=${trace}`, entry.shown], root, scratch, slot)
      : await run(options.jet, ['ir', 'build', `--trace=${trace}`, entry.shown, '-o', exe], root, scratch, slot);
    const binary = build.code === 0 ? (kind === 'old' ? findBuilt(root, basename(entry.path, '.jet')) : exe) : null;
    const executed = binary ? await run(binary, [], root, scratch, slot) : null;
    writeFileSync(join(caseDir, `${kind}.build.err`), build.stderr);
    if (executed) { writeFileSync(join(caseDir, `${kind}.run.out`), executed.stdout); writeFileSync(join(caseDir, `${kind}.run.err`), executed.stderr); }
    rmSync(root, { recursive: true, force: true });
    return { build, binary, executed, phases: phases(trace) };
  };
  const old = await side('old');
  const ir = await side('ir');
  const status = (s) => s.build.timedOut ? 'build-timeout' : s.build.code !== 0 ? `build-exit-${s.build.code}` : !s.binary ? 'no-binary' : s.executed.timedOut ? 'run-timeout' : `run-exit-${s.executed.code ?? `sig-${s.executed.signal}`}`;
  let identical = old.build.code === ir.build.code;
  if (identical && old.build.code !== 0) identical = old.build.stderr.equals(ir.build.stderr);
  if (identical && old.executed && ir.executed) identical = old.executed.code === ir.executed.code && old.executed.signal === ir.executed.signal && old.executed.stdout.equals(ir.executed.stdout) && old.executed.stderr.equals(ir.executed.stderr);
  if (identical && (!old.executed) !== (!ir.executed)) identical = false;
  // The IR side as the golden harness sees a `jet run`: the build's report when it fails, else the run.
  const goldenRun = (s) => s.executed ? { code: s.executed.code, stdout: text(s.executed.stdout), stderr: text(s.executed.stderr), timedOut: s.executed.timedOut, timeoutSec: options.timeout }
    : { code: s.build.code, stdout: text(s.build.stdout), stderr: text(s.build.stderr), timedOut: s.build.timedOut, timeoutSec: options.timeout };
  const irGolden = classifyRun(entry, goldenRun(ir), expected).status;
  const oldGolden = classifyRun(entry, goldenRun(old), expected).status;
  const failures = ir.build.code === 2 ? irFailures(text(ir.build.stderr)) : [];
  if (identical) rmSync(caseDir, { recursive: true, force: true });
  return { shown: entry.shown, old: status(old), ir: status(ir), identical, oldGolden, irGolden, failures, irStderr: firstLine(text(ir.build.stderr)), oldPhases: old.phases, irPhases: ir.phases };
}

async function mapLimit(items, limit, fn) {
  const results = new Array(items.length);
  let next = 0;
  await Promise.all(Array.from({ length: Math.max(1, Math.min(limit, items.length)) }, async (_, slot) => {
    while (next < items.length) { const index = next++; results[index] = await fn(items[index], slot); }
  }));
  return results;
}

const results = await mapLimit(entries, options.jobs, compare);
rmSync(join(options.out, 'scratch'), { recursive: true, force: true });
const fmt = (p) => Object.entries(p).map(([k, v]) => `${k}=${v.toFixed(1)}`).join(';');
writeFileSync(join(options.out, 'differential.tsv'), ['example\told_status\tir_status\tidentical\told_golden\tir_golden\tir_first_failure\told_phases_ms\tir_phases_ms']
  .concat(results.map((r) => [r.shown, r.old, r.ir, r.identical ? 'yes' : 'no', r.oldGolden, r.irGolden, r.failures[0] ?? (r.ir.startsWith('build-exit') ? r.irStderr : '-'), fmt(r.oldPhases), fmt(r.irPhases)].join('\t'))).join('\n') + '\n');

const count = (pred) => results.filter(pred).length;
const lines = [];
const identical = count((r) => r.identical);
const irBuildFailed = count((r) => !r.identical && r.ir.startsWith('build') && !r.old.startsWith('build'));
lines.push(`programs ${results.length}  identical ${identical}  different ${results.length - identical - irBuildFailed}  ir-build-failed ${irBuildFailed}`);
lines.push(`ir golden: passed ${count((r) => r.irGolden === 'passed')}  old golden: passed ${count((r) => r.oldGolden === 'passed')}`);
const histogram = (rows) => { const h = new Map(); for (const g of rows) h.set(g, (h.get(g) ?? 0) + 1); return [...h].sort((a, b) => b[1] - a[1]); };
lines.push('', '# IR build failures by first failure (programs)');
for (const [group, n] of histogram(results.filter((r) => r.failures.length).map((r) => failureGroup(r.failures[0])))) lines.push(`${String(n).padStart(5)}  ${group}`);
lines.push('', '# IR build failures, every row (functions)');
for (const [group, n] of histogram(results.flatMap((r) => r.failures.map(failureGroup)))) lines.push(`${String(n).padStart(5)}  ${group}`);
lines.push('', '# IR route stopped without failure rows (first stderr line)');
for (const [group, n] of histogram(results.filter((r) => !r.identical && r.ir.startsWith('build-exit') && !r.failures.length).map((r) => r.irStderr.slice(0, 160)))) lines.push(`${String(n).padStart(5)}  ${group}`);
lines.push('', '# built by both, different behavior');
for (const r of results.filter((r) => !r.identical && !r.ir.startsWith('build') && !r.old.startsWith('build'))) lines.push(`  ${r.shown}  old ${r.old}  ir ${r.ir}  ir golden ${r.irGolden}`);
lines.push('', '# old fails the golden, IR passes it');
for (const r of results.filter((r) => r.oldGolden !== 'passed' && r.irGolden === 'passed')) lines.push(`  ${r.shown}  old ${r.old} (${r.oldGolden})`);
const sums = (key, subset) => { const t = {}; for (const r of subset) for (const [k, v] of Object.entries(r[key])) t[k] = (t[k] ?? 0) + v; return t; };
const both = results.filter((r) => r.old.startsWith('run') && r.ir.startsWith('run'));
lines.push('', `# per-phase wall ms, summed over the ${both.length} programs both paths built`);
const oldSums = sums('oldPhases', both), irSums = sums('irPhases', both);
for (const name of [...new Set([...Object.keys(oldSums), ...Object.keys(irSums)])].sort()) lines.push(`  ${name.padEnd(28)} old ${(oldSums[name] ?? 0).toFixed(0).padStart(9)}  ir ${(irSums[name] ?? 0).toFixed(0).padStart(9)}`);
const backOld = ['lower', 'assemble', 'optimize', 'native.lower', 'native.object', 'native.link'].reduce((a, k) => a + (oldSums[k] ?? 0), 0);
const backIr = ['lower', 'ir.build', 'ir.x64', 'ir.object', 'ir.link'].reduce((a, k) => a + (irSums[k] ?? 0), 0);
lines.push(`  back end total               old ${backOld.toFixed(0).padStart(9)}  ir ${backIr.toFixed(0).padStart(9)}`);
writeFileSync(join(options.out, 'summary.txt'), lines.join('\n') + '\n');
process.stdout.write(lines.join('\n') + '\n');
process.exitCode = identical === results.length ? 0 : 1;
