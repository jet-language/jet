#!/usr/bin/env node
// Real Jet inline-Atomic source object; link only its independent provider.
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { candidateJetc0 } from '../../JetBackend/Tests/jet-front.mjs';
import { runNativeLibrary } from '../../../Tools/agent/compiler-native-transport.mjs';
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
  const prefix = join(unit, 'fixture');
  const { problem } = runNativeLibrary({ jet: candidateJetc0(), file: join(unit, 'Source/Fixture.jet'), prefix, exports: join(unit, 'exports.list'), env: { ...process.env, JET_TOOLCHAIN_ROOT: root, JET_STORE_DIR: join(out, 'store') } });
  if (problem) throw Error(`candidate front ${problem.verdict}: ${problem.detail}`);
} else if (mode === 'audit') {
  const object = join(unit, 'fixture.o');
  const imports = command('imports', 'nm', ['-u', '--format=posix', object]).stdout;
  if (imports.trim()) throw Error(`Atomic word operations must not import allocation, Rust, locks, or host helpers:\n${imports}`);
  const symbols = command('exports', 'nm', ['-g', '--defined-only', '--format=posix', object]).stdout;
  for (const name of exports) if (symbols.split('\n').filter(line => line.startsWith(`${name} T `)).length !== 1) throw Error(`Atomic provider ${name} must have one strong Jet definition`);
  command('disassembly', 'objdump', ['-dr', object]);
} else if (mode === 'link') {
  command('link', join(root, 'Tools/agent/jet-env'), [nativeCCompiler, '-std=c11', '-O1', '-Wall', '-Wextra', '-Werror', '-pthread', join(root, 'Compiler/JetRuntime/Tests/AtomicTests.c'), join(unit, 'fixture.o'), '-o', join(out, 'atomic-word-fixture')]);
} else {
  const result = command('run', join(out, 'atomic-word-fixture'), [], { timeout: 300000 });
  if (result.stdout !== 'Atomic word: five kinds, wrap/CAS, four writers, SeqCst store-buffering\n' || result.stderr !== '') throw Error('Atomic word witnesses changed exact output');
}
console.log(`Atomic word fixtures ${mode}: ok`);
