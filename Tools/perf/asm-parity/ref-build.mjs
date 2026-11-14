#!/usr/bin/env node
// Builds a hand-written reference (a cell's ref.S) into the executable both
// asm-parity.mjs and the gauntlet harness measure as the machine ceiling:
// `as` assembles it (with this directory on the include path, so a kernel
// cell can .include common.S), `objcopy -O binary` takes its .text, and the
// file is that blob behind an ELF header, one R+X PT_LOAD and PT_GNU_STACK
// (176 bytes): no section headers, symbols or padding.
//
//   node Tools/perf/asm-parity/ref-build.mjs SRC OUT
//
// `as` and `objcopy` come from PATH (Tools/agent/JetEnv.jet provides them).
import { spawnSync } from 'node:child_process';
import { chmodSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));

function must(cmd, args) {
  const r = spawnSync(cmd, args, { encoding: 'buffer', maxBuffer: 1 << 26 });
  if (r.error) throw r.error;
  if (r.status !== 0) throw new Error(`${cmd} ${args.join(' ')} failed (${r.status}):\n${r.stderr.toString()}`);
  return r;
}

// The blob behind an ELF header, one R+X PT_LOAD over the whole file and
// PT_GNU_STACK (read+write, so the stack is never executable).
export function elf(blob) {
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

// `src` assembled into `exe`: its .text behind an ELF header. The object and
// the raw blob are left beside `exe` as `exe`.o and `exe`.bin.
export function assemble(src, exe) {
  const obj = `${exe}.o`, bin = `${exe}.bin`;
  must('as', ['-I', HERE, '-o', obj, src]);
  must('objcopy', ['-O', 'binary', '-j', '.text', obj, bin]);
  writeFileSync(exe, elf(readFileSync(bin)));
  chmodSync(exe, 0o755);
  return exe;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [src, out] = process.argv.slice(2);
  if (!src || !out || process.argv.length !== 4) {
    console.error('usage: node Tools/perf/asm-parity/ref-build.mjs SRC OUT');
    process.exit(64);
  }
  try {
    assemble(resolve(src), resolve(out));
  } catch (error) {
    console.error(`ref-build: ${error.message}`);
    process.exit(1);
  }
}
