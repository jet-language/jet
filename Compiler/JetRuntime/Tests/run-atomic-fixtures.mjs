#!/usr/bin/env node
// Real Jet inline-Atomic source object; link only its independent provider.
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { candidateJetc0, frontReceipt } from '../../JetBackend/Tests/jet-front.mjs';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, mode] = process.argv.slice(2);
if (!output || !['assemble', 'lower', 'audit', 'link', 'run'].includes(mode)) throw Error('usage: run-atomic-fixtures.mjs OUT assemble|lower|audit|link|run');
const out = resolve(output), unit = join(out, 'unit');
const source = readFileSync(join(root, 'Compiler/JetRuntime/Source/Atomic.jet'), 'utf8');
const exports = [...source.matchAll(/^#Export\(c\)\s+fn\s+(\w+)\(/gm)].map(match => match[1]);
function command(name, binary, args, options = {}) {
  const result = spawnSync(binary, args, { cwd: root, encoding: 'utf8', maxBuffer: 1 << 27, ...options });
  writeFileSync(join(out, name + '.receipt.json'), JSON.stringify({ status: result.status, signal: result.signal, error: result.error?.message ?? null, stdout: result.stdout, stderr: result.stderr }, null, 2) + '\n');
  if (result.error || result.status !== 0) throw Error(`${name}: ${result.error?.message ?? result.status}\n${result.stdout ?? ''}${result.stderr ?? ''}`);
  return result;
}
if (mode === 'assemble') {
  mkdirSync(join(unit, 'Source'), { recursive: true });
  if (exports.length !== 24 || new Set(exports).size !== 24) throw Error('expected exactly 24 fixed scalar Atomic providers');
  writeFileSync(join(unit, 'Source/Fixture.jet'), readFileSync(join(root, 'Compiler/JetBackend/Source/OS/Linux.jet'), 'utf8') + '\n' + source);
  writeFileSync(join(unit, 'package.jet'), readFileSync(join(root, 'Compiler/JetRuntime/package.jet')));
  writeFileSync(join(unit, 'exports.list'), exports.sort().join('\n') + '\n');
} else if (mode === 'lower') {
  const prefix = join(unit, 'fixture'), receipt = join(unit, 'front.receipt');
  const result = command('front', candidateJetc0(), [], { timeout: Number(process.env.JET_FRONT_TIMEOUT_MS ?? 600000), env: { ...process.env,
    JET_BOOTSTRAP_MODE: 'factory', JET_BOOTSTRAP_FACTORY_TIER: 'aot', JET_BOOTSTRAP_SOURCE_ROOT: unit,
    JET_BOOTSTRAP_ENTRY: 'Source/Fixture.jet', JET_BOOTSTRAP_OUTPUT: join(unit, 'unused.output'), JET_BOOTSTRAP_RECEIPT: receipt,
    JET_FRONT_LIBRARY_OBJECT_OUTPUT: prefix, JET_FRONT_LIBRARY_EXPORTS: join(unit, 'exports.list'), JET_STORE_DIR: join(out, 'store') } });
  const problem = frontReceipt(existsSync(receipt) ? readFileSync(receipt, 'utf8') : '', result);
  if (problem) throw Error(`candidate front ${problem.verdict}: ${problem.detail}`);
  if (existsSync(prefix + '.issues')) throw Error(readFileSync(prefix + '.issues', 'utf8'));
  if (!existsSync(prefix + '.o')) throw Error('candidate did not emit the real Atomic fixture object');
} else if (mode === 'audit') {
  const object = join(unit, 'fixture.o');
  const imports = command('imports', 'nm', ['-u', '--format=posix', object]).stdout;
  if (imports.trim()) throw Error(`Atomic word operations must not import allocation, Rust, locks, or host helpers:\n${imports}`);
  const symbols = command('exports', 'nm', ['-g', '--defined-only', '--format=posix', object]).stdout;
  for (const name of exports) if (symbols.split('\n').filter(line => line.startsWith(`${name} T `)).length !== 1) throw Error(`Atomic provider ${name} must have one strong Jet definition`);
  command('disassembly', 'objdump', ['-dr', object]);
} else if (mode === 'link') {
  command('link', join(root, 'Tools/agent/jet-env'), [process.env.CC ?? 'cc', '-std=c11', '-O1', '-Wall', '-Wextra', '-Werror', '-pthread', join(root, 'Compiler/JetRuntime/Tests/AtomicTests.c'), join(unit, 'fixture.o'), '-o', join(out, 'atomic-word-fixture')]);
} else {
  const result = command('run', join(out, 'atomic-word-fixture'), [], { timeout: 300000 });
  if (result.stdout !== 'Atomic word: five kinds, wrap/CAS, four writers, SeqCst store-buffering\n' || result.stderr !== '') throw Error('Atomic word witnesses changed exact output');
}
console.log(`Atomic word fixtures ${mode}: ok`);
