#!/usr/bin/env node
// Jet against hand-written x86-64 assembly, in size and in cycles (cards
// #6328 C6 and #6414). Each cell in cells.json is a Jet package and a ref.S
// that does the same work with the same output, written as an expert would.
//
//   node Tools/perf/asm-parity/asm-parity.mjs --jet PATH [--out DIR] [--runs N]
//       [--cpu C] [--modes default,release] [--cells hello,fib] [--perf PATH] [--gate]
//
// `--jet` must run with the compiler's environment already set
// (JET_TOOLCHAIN_ROOT, JET_COMPILER_IDENTITY); `as`, `objcopy` and `perf`
// come from PATH (Tools/agent/jet-env provides the first two).
//
// Per cell:
// - the reference: `as` assembles ref.S (common.S is .include'd first, so
//   _start is the blob's first byte), `objcopy -O binary` takes its .text,
//   and the file is that blob behind an ELF header, one R+X PT_LOAD and
//   PT_GNU_STACK (176 bytes): no section headers, symbols or padding;
// - the Jet program: `jet build` of a copy of the package, once per mode
//   (default, release = --release);
// - output: every binary's stdout must equal the cell's `expect`
//   byte for byte, with exit status 0 (exit 1 otherwise);
// - size: file bytes, and Jet / reference;
// - speed: `perf stat -e cycles:u,instructions:u`, best of N runs (default
//   5) pinned to one CPU (default 8), and Jet / reference.
// Gates: size <= 2.00x the reference (#6328 C6) and cycles <= 1.10x
// (#6414). They are reported per row; `--gate` also exits 1 when one fails.
//
// Writes DIR/results.json and DIR/table.tsv (default DIR: .asm-parity under
// the current directory) and prints the table.
import { spawnSync } from 'node:child_process';
import { chmodSync, cpSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const SIZE_GATE = 2.0;
const CYCLES_GATE = 1.10;

function options(argv) {
  const o = { out: resolve('.asm-parity'), runs: 5, cpu: '8', modes: ['default', 'release'], cells: null, perf: 'perf', gate: false, jet: null };
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
    else throw new Error(`unknown argument ${a}`);
  }
  if (!o.jet) throw new Error('--jet PATH is required');
  for (const m of o.modes) if (m !== 'default' && m !== 'release') throw new Error(`unknown mode ${m}`);
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

function buildRef(cell, dir) {
  const src = join(HERE, cell.name, 'ref.S');
  const obj = join(dir, 'ref.o'), bin = join(dir, 'ref.bin'), exe = join(dir, 'ref');
  must('as', ['-I', HERE, '-o', obj, src]);
  must('objcopy', ['-O', 'binary', '-j', '.text', obj, bin]);
  writeFileSync(exe, elf(readFileSync(bin)));
  chmodSync(exe, 0o755);
  return exe;
}

function buildJet(o, cell, mode, dir) {
  const src = join(dir, `src-${mode}`);
  rmSync(src, { recursive: true, force: true });
  cpSync(resolve(HERE, cell.jet), src, { recursive: true, filter: (p) => !p.includes('/.jet') });
  const args = mode === 'release' ? ['build', '--release', '.'] : ['build', '.'];
  must(o.jet, args, { cwd: src });
  return join(src, '.jet', 'build', 'run');
}

function output(exe, args) {
  const r = run(exe, args);
  return { stdout: r.stdout.toString('latin1'), status: r.status, signal: r.signal };
}

// Best of `runs` (fewest cycles) user-mode cycles and instructions.
function measure(o, exe, args) {
  let best = null;
  for (let i = 0; i < o.runs; i++) {
    const r = run('taskset', ['-c', o.cpu, o.perf, 'stat', '-x,', '-e', 'cycles:u,instructions:u', exe, ...args]);
    const row = {};
    for (const line of r.stderr.toString().split('\n')) {
      const f = line.split(',');
      if (f[2] === 'cycles:u' || f[2] === 'instructions:u') row[f[2].split(':')[0]] = Number(f[0]);
    }
    if (!Number.isFinite(row.cycles)) throw new Error(`perf stat gave no cycles for ${exe}:\n${r.stderr.toString()}`);
    if (!best || row.cycles < best.cycles) best = row;
  }
  return best;
}

const o = options(process.argv.slice(2));
const cells = JSON.parse(readFileSync(join(HERE, 'cells.json'), 'utf8')).filter((c) => !o.cells || o.cells.includes(c.name));
mkdirSync(o.out, { recursive: true });
const rows = [];
let wrong = 0, gated = 0;
for (const cell of cells) {
  const dir = join(o.out, cell.name);
  mkdirSync(dir, { recursive: true });
  const variants = [{ variant: 'asm', exe: buildRef(cell, dir) }];
  for (const mode of o.modes) variants.push({ variant: `jet-${mode}`, exe: buildJet(o, cell, mode, dir) });
  let ref = null;
  for (const v of variants) {
    const out = output(v.exe, cell.args);
    const same = out.status === 0 && out.stdout === cell.expect;
    if (!same) wrong++;
    const speed = measure(o, v.exe, cell.args);
    const bytes = statSync(v.exe).size;
    const row = { cell: cell.name, variant: v.variant, args: cell.args.join(' '), bytes, cycles: speed.cycles, instructions: speed.instructions, output: same ? 'same' : `DIFF (status ${out.status}${out.signal ? ` ${out.signal}` : ''})` };
    if (!ref) ref = row;
    row.size_x = bytes / ref.bytes;
    row.cycles_x = speed.cycles / ref.cycles;
    row.size_gate = row.size_x <= SIZE_GATE ? 'ok' : 'over';
    row.cycles_gate = row.cycles_x <= CYCLES_GATE ? 'ok' : 'over';
    if (v.variant !== 'asm' && (row.size_gate !== 'ok' || row.cycles_gate !== 'ok')) gated++;
    rows.push(row);
  }
}
const cols = ['cell', 'variant', 'args', 'bytes', 'size_x', 'size_gate', 'cycles', 'instructions', 'cycles_x', 'cycles_gate', 'output'];
const fmt = (r, c) => (typeof r[c] === 'number' && !Number.isInteger(r[c]) ? r[c].toFixed(2) : String(r[c]));
const table = [cols.join('\t'), ...rows.map((r) => cols.map((c) => fmt(r, c)).join('\t'))].join('\n') + '\n';
writeFileSync(join(o.out, 'results.json'), JSON.stringify({ runs: o.runs, cpu: o.cpu, gates: { size: SIZE_GATE, cycles: CYCLES_GATE }, rows }, null, 2) + '\n');
writeFileSync(join(o.out, 'table.tsv'), table);
process.stdout.write(table);
if (wrong) console.error(`${wrong} binaries printed the wrong output`);
if (o.gate && gated) console.error(`${gated} Jet rows miss a gate`);
process.exit(wrong || (o.gate && gated) ? 1 : 0);
