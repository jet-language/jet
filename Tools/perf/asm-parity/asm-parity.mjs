#!/usr/bin/env node
// Jet against hand-written x86-64 assembly, in size and in cycles (cards
// #6328 C6 and #6414). Each cell in cells.json is a Jet package and a ref.S
// that does the same work with the same output, written as an expert would.
// A cell's `gauntlet` field names the gauntlet matrix cells whose machine
// ceiling it is (AGENTS.md "Performance gate"): the gauntlet harness reads
// this file and judges those cells Jet/ceiling <= 1.00 from results.json
// (`run.mjs --ceiling-results`); a required cell no reference names is
// uncovered and fails the gauntlet gate. Its `scriptbench` field names the
// ScriptBench workloads (Tools/perf/scriptbench/manifest.json ids) it bounds
// the same way; an id the manifest lacks fails the run before anything is
// built.
//
//   node Tools/perf/asm-parity/asm-parity.mjs --jet PATH [--out DIR] [--runs N]
//       [--cpu C] [--modes default,release] [--cells hello,fib] [--perf PATH] [--gate]
//
// `--jet` must run with the compiler's environment already set
// (JET_TOOLCHAIN_ROOT, JET_COMPILER_IDENTITY); `as`, `objcopy` and `perf`
// come from PATH (Tools/agent/JetEnv.jet provides the first two).
//
// Per cell:
// - the reference: `as` assembles ref.S (a kernel cell .include's common.S
//   first; either way _start is the blob's first byte), `objcopy -O binary`
//   takes its .text, and the file is that blob behind an ELF header, one R+X
//   PT_LOAD and PT_GNU_STACK (176 bytes): no section headers, symbols or
//   padding;
// - the Jet program: `jet build` of a copy of the package, once per mode
//   (default, release = --release);
// - input: a cell with a `fixture` {script, out} has `python3 script out`
//   write its input (a file or a directory) into the cell's output
//   directory, and every binary runs there (its `args` name it). The input
//   is written once, or, with `fresh: true` (a program that changes it),
//   again before every run. A cell with `server` {script, port, delay_ms}
//   has `node script port delay_ms` serve it: started before the cell's
//   first run, waited for until the port accepts, stopped after the cell;
//   its port and delay must be those of the ScriptBench workloads it names.
//   A cell with `steps` (argument lists) runs them
//   in order in a `work` directory emptied before every run, in place of
//   `args` (a program that keeps state between invocations); stdout is the
//   steps' together and cycles their sum;
// - output: every binary's stdout must equal the cell's `expect` (or the
//   file `expect_file`) byte for byte, with exit status 0 (exit 1
//   otherwise);
// - size: file bytes, and Jet / reference;
// - speed: `perf stat -e cycles:u,instructions:u`, N runs (default 21) of
//   every binary of the cell interleaved (run i of each binary before run
//   i + 1 of any), pinned to one CPU (default 8): the table's cycles and
//   instructions are each the median over the runs, so that a quiet or
//   noisy stretch of the machine lands on every binary alike, and the
//   ratios are Jet / reference of those medians. Instructions, which the
//   machine's state barely moves, are the ratio to read when cycles
//   disagree with them;
// Gates: size <= 2.00x the reference (#6328 C6) and cycles <= 1.10x
// (#6414): the cards' interim bounds, not the final target. They are reported
// per row; `--gate` also exits 1 when one fails.
//
// Writes DIR/results.json and DIR/table.tsv (default DIR: .asm-parity under
// the current directory) and prints the table.
//
// Gains mode (#6414): `--candidate PATH` compares two compilers instead of
// gating one:
//
//   node Tools/perf/asm-parity/asm-parity.mjs --jet BASE --candidate CAND
//       [--older PATH] [--best N] [--gains FILE] [--out DIR] [--cpu C]
//       [--modes release] [--cells sieve,wordfreq]
//
// Every cell of cells.json (with its reference, the hand-asm ceiling) and of
// gains.json (gauntlet programs with no reference: string churn and index
// loops the asm cells lack) is built by BASE and CAND (and, with `--older`,
// by that compiler too, a column showing what BASE gained over it). The base
// binary also runs a second time as a control variant, whose change against
// base is the noise floor. Per binary: output the cell's expected bytes with
// status 0 (exit 1 otherwise); wall time, the best of N runs (default 5),
// each round every variant once back to back (odd rounds reversed) on the
// pinned CPU; peak RSS (the run's
// largest ru_maxrss, read through launch.S); and the stripped size (the file
// bytes the PT_LOAD segments map). It uses `python3` and `taskset`, not
// `perf`. Writes DIR/gains.json and a Markdown table per mode (FILE, default
// DIR/gains.md): each program's base, candidate and ceiling, the candidate's
// change against base in percent, and totals (summed time, RSS and bytes,
// and the geometric mean of candidate / base). Best-of-N wall time is only as
// quiet as the machine: run it with nothing else heavy running.
import { spawn, spawnSync } from 'node:child_process';
import { chmodSync, cpSync, existsSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const SIZE_GATE = 2.0;
const CYCLES_GATE = 1.10;
// A median of fewer runs lets one quiet stretch of the machine decide a row.
const MIN_RUNS = 21;

function options(argv) {
  const o = { out: resolve('.asm-parity'), runs: MIN_RUNS, cpu: '8', modes: ['default', 'release'], cells: null, perf: 'perf', gate: false, jet: null, candidate: null, older: null, best: 5, gains: null };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const value = () => { if (i + 1 >= argv.length) throw new Error(`${a} needs a value`); return argv[++i]; };
    if (a === '--jet') o.jet = resolve(value());
    else if (a === '--out') o.out = resolve(value());
    else if (a === '--runs') o.runs = Number(value());
    else if (a === '--cpu') o.cpu = value();
    else if (a === '--modes') o.modes = value().split(',');
    else if (a === '--cells') o.cells = value().split(',');
    else if (a === '--perf') o.perf = value();
    else if (a === '--gate') o.gate = true;
    else if (a === '--candidate') o.candidate = resolve(value());
    else if (a === '--older') o.older = resolve(value());
    else if (a === '--best') o.best = Number(value());
    else if (a === '--gains') o.gains = resolve(value());
    else throw new Error(`unknown argument ${a}`);
  }
  if (!o.jet) throw new Error('--jet PATH is required');
  if (!Number.isInteger(o.runs) || o.runs < MIN_RUNS) throw new Error(`--runs must be an integer of at least ${MIN_RUNS}`);
  for (const m of o.modes) if (m !== 'default' && m !== 'release') throw new Error(`unknown mode ${m}`);
  if (!o.candidate && (o.older || o.gains)) throw new Error('--older and --gains need --candidate');
  if (!Number.isInteger(o.best) || o.best < 1) throw new Error('--best must be a positive integer');
  return o;
}

function run(cmd, args, opts = {}) {
  const r = spawnSync(cmd, args, { encoding: 'buffer', maxBuffer: 1 << 26, ...opts });
  if (r.error) throw r.error;
  return r;
}

function must(cmd, args, opts = {}) {
  const r = run(cmd, args, opts);
  if (r.status !== 0) throw new Error(`${cmd} ${args.join(' ')} failed (${r.status}):\n${r.stderr.toString()}`);
  return r;
}

// The blob behind an ELF header, one R+X PT_LOAD over the whole file and
// PT_GNU_STACK (read+write, so the stack is never executable).
function elf(blob) {
  const base = 0x400000, ehsize = 64, phsize = 56, phnum = 2;
  const head = ehsize + phnum * phsize;
  const total = head + blob.length;
  const b = Buffer.alloc(total);
  b.write('\x7fELF', 0, 'latin1');
  b[4] = 2; b[5] = 1; b[6] = 1;
  b.writeUInt16LE(2, 16); b.writeUInt16LE(0x3e, 18); b.writeUInt32LE(1, 20);
  b.writeBigUInt64LE(BigInt(base + head), 24);
  b.writeBigUInt64LE(BigInt(ehsize), 32);
  b.writeUInt16LE(ehsize, 52); b.writeUInt16LE(phsize, 54); b.writeUInt16LE(phnum, 56);
  b.writeUInt16LE(64, 58);
  let p = ehsize;
  b.writeUInt32LE(1, p); b.writeUInt32LE(5, p + 4);
  b.writeBigUInt64LE(BigInt(base), p + 16); b.writeBigUInt64LE(BigInt(base), p + 24);
  b.writeBigUInt64LE(BigInt(total), p + 32); b.writeBigUInt64LE(BigInt(total), p + 40);
  b.writeBigUInt64LE(0x1000n, p + 48);
  p += phsize;
  b.writeUInt32LE(0x6474e551, p); b.writeUInt32LE(6, p + 4);
  b.writeBigUInt64LE(16n, p + 48);
  blob.copy(b, head);
  return b;
}

// `src` assembled into DIR/NAME: its .text behind an ELF header.
function assemble(src, dir, name) {
  const obj = join(dir, `${name}.o`), bin = join(dir, `${name}.bin`), exe = join(dir, name);
  must('as', ['-I', HERE, '-o', obj, src]);
  must('objcopy', ['-O', 'binary', '-j', '.text', obj, bin]);
  writeFileSync(exe, elf(readFileSync(bin)));
  chmodSync(exe, 0o755);
  return exe;
}

const buildRef = (cell, dir) => assemble(join(HERE, cell.name, 'ref.S'), dir, 'ref');

function buildJet(jet, cell, mode, dir, label = 'jet') {
  const src = join(dir, label === 'jet' ? `src-${mode}` : `src-${label}-${mode}`);
  rmSync(src, { recursive: true, force: true });
  cpSync(resolve(HERE, cell.jet), src, { recursive: true, filter: (p) => !p.includes('/.jet') });
  const args = mode === 'release' ? ['build', '--release', '.'] : ['build', '.'];
  must(jet, args, { cwd: src });
  return join(src, '.jet', 'build', 'run');
}

// One run: the input prepared, then each step (an argument list) in order;
// stdout is the steps' concatenated, the status the first nonzero one.
function output(exe, plan, cwd, prepare) {
  prepare();
  let stdout = '';
  for (const args of plan) {
    const r = run(exe, args, { cwd });
    stdout += r.stdout.toString('latin1');
    if (r.status !== 0 || r.signal) return { stdout, status: r.status, signal: r.signal };
  }
  return { stdout, status: 0, signal: null };
}

// One run's user-mode cycles and instructions, summed over its steps.
function sample(o, exe, plan, cwd, prepare) {
  prepare();
  const total = { cycles: 0, instructions: 0 };
  for (const args of plan) {
    const r = run('taskset', ['-c', o.cpu, o.perf, 'stat', '-x,', '-e', 'cycles:u,instructions:u', exe, ...args], { cwd });
    const row = {};
    for (const line of r.stderr.toString().split('\n')) {
      const f = line.split(',');
      if (f[2] === 'cycles:u' || f[2] === 'instructions:u') row[f[2].split(':')[0]] = Number(f[0]);
    }
    if (!Number.isFinite(row.cycles) || !Number.isFinite(row.instructions)) throw new Error(`perf stat gave no cycles or instructions for ${exe}:\n${r.stderr.toString()}`);
    total.cycles += row.cycles;
    total.instructions += row.instructions;
  }
  return total;
}

const median = (values) => {
  const sorted = [...values].sort((a, b) => a - b);
  const mid = sorted.length >> 1;
  return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
};

// `runs` rounds over the variants, each round one run of every variant in
// turn; per variant the median cycles and instructions and the cycles' range.
function measure(o, variants, plan, cwd, prepare) {
  const samples = variants.map(() => []);
  for (let i = 0; i < o.runs; i++) {
    variants.forEach((v, k) => samples[k].push(sample(o, v.exe, plan, cwd, prepare)));
  }
  return samples.map((s) => {
    const cycles = s.map((x) => x.cycles);
    return { cycles: median(cycles), instructions: median(s.map((x) => x.instructions)), cycles_min: Math.min(...cycles), cycles_max: Math.max(...cycles) };
  });
}

// Whether 127.0.0.1:port accepts a connection within `ms` milliseconds.
function accepts(port, ms) {
  const probe = `const net = require('node:net'); const end = Date.now() + ${ms};
const once = () => { const s = net.connect(${port}, '127.0.0.1', () => { s.destroy(); process.exit(0); });
  s.on('error', () => { s.destroy(); if (Date.now() >= end) process.exit(1); setTimeout(once, 10); }); };
once();`;
  return run(process.execPath, ['-e', probe]).status === 0;
}

// A cell's server, running once its port accepts; null for a cell without one.
function startServer(cell) {
  if (!cell.server) return null;
  const { script, port, delay_ms } = cell.server;
  if (accepts(port, 0)) throw new Error(`${cell.name}: port ${port} is already served; stop that server first`);
  const child = spawn(process.execPath, [resolve(HERE, script), String(port), String(delay_ms)], { stdio: 'ignore' });
  if (!accepts(port, 10000)) {
    child.kill();
    throw new Error(`${cell.name}: ${script} did not accept on port ${port}`);
  }
  return child;
}

// Every cell's `scriptbench` ids are manifest workloads, and a cell's server
// is the one those workloads use.
function scriptbenchIssues(all) {
  const workloads = new Map(JSON.parse(readFileSync(join(HERE, '..', 'scriptbench', 'manifest.json'), 'utf8')).workloads.map((w) => [w.id, w]));
  const issues = [];
  for (const cell of all) {
    if (cell.scriptbench === undefined) continue;
    if (!Array.isArray(cell.scriptbench)) { issues.push(`${cell.name}: scriptbench is not a list of workload ids`); continue; }
    for (const id of cell.scriptbench) {
      const w = workloads.get(id);
      if (!w) { issues.push(`${cell.name}: scriptbench names unknown workload ${id}`); continue; }
      const servers = w.servers ?? [];
      if (servers.length === 0 && cell.server) issues.push(`${cell.name}: has a server but workload ${id} uses none`);
      for (const s of servers) {
        if (!cell.server || cell.server.port !== s.port || cell.server.delay_ms !== s.delay_ms) {
          issues.push(`${cell.name}: workload ${id} is served on port ${s.port} with a ${s.delay_ms} ms delay; the cell's server differs`);
        }
      }
    }
  }
  return issues;
}

// A cell's directory, working directory, run plan, input preparation and
// expected stdout.
function setup(cell) {
  const dir = join(o.out, cell.name);
  mkdirSync(dir, { recursive: true });
  const input = cell.fixture ? join(dir, cell.fixture.out) : null;
  // A cell with `steps` runs them in `work`, empty at the start of each run.
  const cwd = cell.steps ? join(dir, 'work') : dir;
  const plan = cell.steps ?? [cell.args];
  const prepare = () => {
    if (cell.steps) {
      rmSync(cwd, { recursive: true, force: true });
      mkdirSync(cwd);
    }
    if (!input || (!cell.fixture.fresh && existsSync(input))) return;
    rmSync(input, { recursive: true, force: true });
    must('python3', [resolve(HERE, cell.fixture.script), input]);
  };
  const expect = cell.expect_file ? readFileSync(resolve(HERE, cell.expect_file), 'latin1') : cell.expect;
  return { dir, cwd, plan, prepare, expect };
}

// One run of `exe` with `args` through the launcher (launch.S), pinned to the
// CPU, its stdout dropped: wall nanoseconds from fork to reap, the child's
// ru_maxrss in KiB and its exit code (negative: the signal).
const TIMED = `import os, struct, sys, time
launcher, argv = sys.argv[1], sys.argv[1:]
r, w = os.pipe()
start = time.perf_counter_ns()
pid = os.fork()
if pid == 0:
    try:
        null = os.open(os.devnull, os.O_WRONLY)
        os.dup2(null, 1)
        os.dup2(w, 3)
        os.set_inheritable(3, True)
        os.execv(launcher, argv)
    finally:
        os._exit(127)
os.waitpid(pid, 0)
ns = time.perf_counter_ns() - start
os.close(w)
data = b''
while len(data) < 152:
    more = os.read(r, 152 - len(data))
    if not more:
        sys.exit('launcher wrote no wait status')
    data += more
status, maxrss = struct.unpack_from('<i', data, 0)[0], struct.unpack_from('<q', data, 40)[0]
print(ns, maxrss, os.waitstatus_to_exitcode(status))`;

// One run of a plan: wall time summed over its steps, the largest peak RSS,
// and the first nonzero exit code.
function timed(launcher, exe, plan, cwd, prepare) {
  prepare();
  const total = { ns: 0, rss: 0, code: 0 };
  for (const args of plan) {
    const r = must('taskset', ['-c', o.cpu, 'python3', '-c', TIMED, launcher, exe, ...args], { cwd });
    const [ns, rss, code] = r.stdout.toString().trim().split(' ').map(Number);
    total.ns += ns;
    total.rss = Math.max(total.rss, rss);
    if (code !== 0 && total.code === 0) total.code = code;
  }
  return total;
}

// The stripped size: the file bytes up to the end of the last PT_LOAD
// segment, all a stripped binary keeps. (`objcopy --strip-all` cannot
// measure it: it rebuilds a file from the section headers, and a Jet
// binary's few section headers do not cover its segments.)
function stripped(exe) {
  const b = readFileSync(exe);
  const phoff = Number(b.readBigUInt64LE(32)), phsize = b.readUInt16LE(54), phnum = b.readUInt16LE(56);
  let end = phoff + phsize * phnum;
  for (let i = 0; i < phnum; i++) {
    const p = phoff + i * phsize;
    if (b.readUInt32LE(p) === 1) end = Math.max(end, Number(b.readBigUInt64LE(p + 8) + b.readBigUInt64LE(p + 32)));
  }
  return end;
}

const pct = (to, from) => (from === 0 ? '-' : `${to >= from ? '+' : ''}${((to / from - 1) * 100).toFixed(1)}%`);
const ms = (ns) => (ns / 1e6).toFixed(1);
const geomean = (xs) => Math.exp(xs.reduce((s, x) => s + Math.log(x), 0) / xs.length);

// Gains mode: every cell built by the base, candidate (and older) compiler,
// against the hand-asm ceiling where the cell has one.
function gains(all) {
  const extra = JSON.parse(readFileSync(join(HERE, 'gains.json'), 'utf8'));
  const cells = [...all, ...extra].filter((c) => !o.cells || o.cells.includes(c.name));
  const compilers = [...(o.older ? [['older', o.older]] : []), ['base', o.jet], ['candidate', o.candidate]];
  const launcher = assemble(join(HERE, 'launch.S'), o.out, 'launch');
  const results = [];
  let wrong = 0;
  for (const mode of o.modes) {
    for (const cell of cells) {
      const { dir, cwd, plan, prepare, expect } = setup(cell);
      const variants = [];
      if (existsSync(join(HERE, cell.name, 'ref.S'))) variants.push({ label: 'ceiling', exe: buildRef(cell, dir) });
      for (const [label, jet] of compilers) variants.push({ label, exe: buildJet(jet, cell, mode, dir, label) });
      // The control is the base binary measured again as its own variant: its
      // change against base is the run's noise floor.
      variants.push({ label: 'control', exe: variants.find((v) => v.label === 'base').exe });
      const server = startServer(cell);
      const runs = variants.map(() => []);
      let outs;
      try {
        outs = variants.map((v) => output(v.exe, plan, cwd, prepare));
        // Each round runs every variant once, back to back (ABAB...); odd
        // rounds reverse the order, so no variant always runs first.
        const order = variants.map((_, k) => k);
        for (let i = 0; i < o.best; i++) for (const k of i % 2 ? [...order].reverse() : order) runs[k].push(timed(launcher, variants[k].exe, plan, cwd, prepare));
      } finally {
        server?.kill();
      }
      const row = { mode, cell: cell.name, args: cell.steps ? cell.steps.map((s) => s.join(' ')).join(' ; ') : cell.args.join(' '), wrong: [] };
      variants.forEach((v, k) => {
        const same = outs[k].status === 0 && outs[k].stdout === expect && runs[k].every((r) => r.code === 0);
        if (!same) { wrong++; row.wrong.push(v.label); }
        row[v.label] = { ns: Math.min(...runs[k].map((r) => r.ns)), rss: Math.max(...runs[k].map((r) => r.rss)), bytes: stripped(v.exe), runs: runs[k] };
      });
      results.push(row);
      console.error(`${mode} ${cell.name}: base ${ms(row.base.ns)} ms, candidate ${ms(row.candidate.ns)} ms (${pct(row.candidate.ns, row.base.ns)}), control ${pct(row.control.ns, row.base.ns)}${row.wrong.length ? `, WRONG OUTPUT: ${row.wrong.join(', ')}` : ''}`);
    }
  }
  writeFileSync(join(o.out, 'gains.json'), JSON.stringify({ best_of: o.best, interleaved: true, cpu: o.cpu, compilers: Object.fromEntries(compilers), rows: results }, null, 2) + '\n');
  const lines = [`# Codegen gains: base against candidate (#6414)`, '', `Base: \`${o.jet}\`. Candidate: \`${o.candidate}\`.${o.older ? ` Older: \`${o.older}\`.` : ''}`, `Wall time is the best of ${o.best} runs, taken in rounds of every variant back to back (odd rounds in reverse order) on CPU ${o.cpu}. Control is the base binary measured again as its own variant: its change against base is the noise floor, and a candidate change inside it is not a gain. RSS is the largest peak (ru_maxrss) of those runs; Linux counts it in per-CPU batches, so a peak under a few hundred KiB can read 0. Bytes are the stripped size (file bytes the PT_LOAD segments map). Ceiling is the hand-written assembly reference (asm-parity cells only). Change is candidate against base; negative is better.`, ''];
  for (const mode of o.modes) {
    const rows = results.filter((r) => r.mode === mode);
    const older = o.older ? ['older ms', 'base vs older'] : [];
    const head = ['program', ...older, 'base ms', 'candidate ms', 'change', 'control ms', 'control vs base', 'ceiling ms', 'candidate / ceiling', 'base RSS KiB', 'candidate RSS KiB', 'change', 'ceiling RSS KiB', 'base bytes', 'candidate bytes', 'change', 'ceiling bytes', 'output'];
    lines.push(`## ${mode}`, '', `| ${head.join(' | ')} |`, `|${head.map((h, i) => (i === 0 || h === 'output' ? ' --- ' : ' ---: ')).join('|')}|`);
    for (const r of rows) {
      const c = r.ceiling;
      lines.push(`| ${[r.cell, ...(o.older ? [ms(r.older.ns), pct(r.base.ns, r.older.ns)] : []), ms(r.base.ns), ms(r.candidate.ns), pct(r.candidate.ns, r.base.ns), ms(r.control.ns), pct(r.control.ns, r.base.ns), c ? ms(c.ns) : '-', c ? (r.candidate.ns / c.ns).toFixed(2) : '-', r.base.rss, r.candidate.rss, pct(r.candidate.rss, r.base.rss), c ? c.rss : '-', r.base.bytes, r.candidate.bytes, pct(r.candidate.bytes, r.base.bytes), c ? c.bytes : '-', r.wrong.length ? `WRONG: ${r.wrong.join(', ')}` : 'same'].join(' | ')} |`);
    }
    const sum = (label, key) => rows.reduce((s, r) => s + r[label][key], 0);
    // Rows with a zero (an RSS under the kernel's counting batch) have no ratio.
    const ratios = (key, to, from) => rows.filter((r) => r[from] && r[to][key] > 0 && r[from][key] > 0).map((r) => r[to][key] / r[from][key]);
    const ratio = (key, to = 'candidate', from = 'base') => { const xs = ratios(key, to, from); return xs.length ? pct(geomean(xs), 1) : '-'; };
    const toCeiling = ratios('ns', 'candidate', 'ceiling');
    lines.push(`| **total** | ${[...(o.older ? [ms(sum('older', 'ns')), pct(sum('base', 'ns'), sum('older', 'ns'))] : []), ms(sum('base', 'ns')), ms(sum('candidate', 'ns')), pct(sum('candidate', 'ns'), sum('base', 'ns')), ms(sum('control', 'ns')), pct(sum('control', 'ns'), sum('base', 'ns')), '', '', sum('base', 'rss'), sum('candidate', 'rss'), pct(sum('candidate', 'rss'), sum('base', 'rss')), '', sum('base', 'bytes'), sum('candidate', 'bytes'), pct(sum('candidate', 'bytes'), sum('base', 'bytes')), '', ''].join(' | ')} |`);
    lines.push(`| **geometric mean** | ${[...(o.older ? ['', ratio('ns', 'base', 'older')] : []), '', '', ratio('ns'), '', ratio('ns', 'control'), '', toCeiling.length ? geomean(toCeiling).toFixed(2) : '-', '', '', ratio('rss'), '', '', '', ratio('bytes'), '', ''].join(' | ')} |`, '');
  }
  const doc = lines.join('\n');
  writeFileSync(o.gains ?? join(o.out, 'gains.md'), doc);
  process.stdout.write(doc);
  if (wrong) console.error(`${wrong} binaries printed the wrong output`);
  process.exit(wrong ? 1 : 0);
}

const o = options(process.argv.slice(2));
const all = JSON.parse(readFileSync(join(HERE, 'cells.json'), 'utf8'));
const issues = scriptbenchIssues(all);
if (issues.length) {
  for (const issue of issues) console.error(`cells.json: ${issue}`);
  process.exit(1);
}
mkdirSync(o.out, { recursive: true });
if (o.candidate) gains(all);
const cells = all.filter((c) => !o.cells || o.cells.includes(c.name));
const rows = [];
let wrong = 0, gated = 0;
for (const cell of cells) {
  const { dir, cwd, plan, prepare, expect } = setup(cell);
  const variants = [{ variant: 'asm', exe: buildRef(cell, dir) }];
  for (const mode of o.modes) variants.push({ variant: `jet-${mode}`, exe: buildJet(o.jet, cell, mode, dir) });
  const server = startServer(cell);
  let outs, speeds;
  try {
    outs = variants.map((v) => output(v.exe, plan, cwd, prepare));
    speeds = measure(o, variants, plan, cwd, prepare);
  } finally {
    server?.kill();
  }
  let ref = null;
  variants.forEach((v, k) => {
    const out = outs[k], speed = speeds[k];
    const same = out.status === 0 && out.stdout === expect;
    if (!same) wrong++;
    const bytes = statSync(v.exe).size;
    const args = cell.steps ? cell.steps.map((s) => s.join(' ')).join(' ; ') : cell.args.join(' ');
    const row = { cell: cell.name, variant: v.variant, args, bytes, ...speed, output: same ? 'same' : `DIFF (status ${out.status}${out.signal ? ` ${out.signal}` : ''})` };
    if (!ref) ref = row;
    row.size_x = bytes / ref.bytes;
    row.cycles_x = speed.cycles / ref.cycles;
    row.instructions_x = speed.instructions / ref.instructions;
    row.size_gate = row.size_x <= SIZE_GATE ? 'ok' : 'over';
    row.cycles_gate = row.cycles_x <= CYCLES_GATE ? 'ok' : 'over';
    if (v.variant !== 'asm' && (row.size_gate !== 'ok' || row.cycles_gate !== 'ok')) gated++;
    rows.push(row);
  });
}
const cols = ['cell', 'variant', 'args', 'bytes', 'size_x', 'size_gate', 'cycles', 'instructions', 'cycles_x', 'instructions_x', 'cycles_gate', 'output'];
const fmt = (r, c) => (typeof r[c] === 'number' && !Number.isInteger(r[c]) ? r[c].toFixed(2) : String(r[c]));
const table = [cols.join('\t'), ...rows.map((r) => cols.map((c) => fmt(r, c)).join('\t'))].join('\n') + '\n';
writeFileSync(join(o.out, 'results.json'), JSON.stringify({ runs: o.runs, interleaved: true, statistic: 'median', cpu: o.cpu, gates: { size: SIZE_GATE, cycles: CYCLES_GATE }, rows }, null, 2) + '\n');
writeFileSync(join(o.out, 'table.tsv'), table);
process.stdout.write(table);
if (wrong) console.error(`${wrong} binaries printed the wrong output`);
if (o.gate && gated) console.error(`${gated} Jet rows miss a gate`);
process.exit(wrong || (o.gate && gated) ? 1 : 0);
