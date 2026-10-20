#!/usr/bin/env node
// Same full-object convention as run-scheduler-fixtures.mjs. Runtime and Jet
// callbacks are lowered together; the C witness only transports code pointers.
// Aggregate fixture.o supplies the Jet-only runtime; link platform libs only.
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { candidateJetc0 } from '../../JetBackend/Tests/jet-front.mjs';
import { runNativeLibrary } from '../../../Tools/agent/compiler-native-transport.mjs';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, mode] = process.argv.slice(2);
if (!output || !['assemble', 'lower', 'audit', 'link', 'run'].includes(mode)) throw Error('usage: run-testsuite-fixtures.mjs OUT assemble|lower|audit|link|run');
const out = resolve(output), dir = join(out, 'unit');
function command(bin, args, options = {}) {
  const result = spawnSync(bin, args, { encoding: 'utf8', maxBuffer: 1 << 27, ...options });
  if (result.error || result.status !== 0) throw Error(`${bin}: ${result.status ?? result.signal}\n${result.error?.message ?? ''}\n${result.stdout ?? ''}${result.stderr ?? ''}`);
  return result;
}
if (mode === 'assemble') {
  command(process.execPath, [join(root, 'Compiler/JetRuntime/Tests/check-runtime.mjs'), dir, '--assemble-only', '--test', 'Test']);
  mkdirSync(join(dir, 'Source'), { recursive: true });
  writeFileSync(join(dir, 'Source/Fixture.jet'), readFileSync(join(dir, 'unit.jet'), 'utf8'));
  const entries = new Map(readFileSync(join(root, 'Compiler/JetRuntime/exports.list'), 'utf8').split('\n')
    .filter(x => x && !x.startsWith('#')).map(x => [x.split(/\s+/)[0], x]));
  // Scratch-only export admission for the held witness; never edit the frozen
  // runtime manifest. Once regenerated, the same rows are merely reused.
  for (const path of ['Source/TestSuite.jet', 'Tests/TestSuiteTests.jet']) {
    const source = readFileSync(join(root, 'Compiler/JetRuntime', path), 'utf8');
    for (const match of source.matchAll(/^#Export\(c\)\s+fn\s+(\w+)\(/gm)) {
      const name = match[1];
      entries.set(name, name === 'jet_rt_test_suite_init' ? `${name} init 3` : name);
    }
  }
  writeFileSync(join(dir, 'exports.list'), [...entries.values()].sort().join('\n') + '\n');
} else if (mode === 'lower') {
  const prefix = join(dir, 'fixture');
  const {problem} = runNativeLibrary({jet:candidateJetc0(),file:join(dir,'Source/Fixture.jet'),prefix,exports:join(dir,'exports.list'),env:{...process.env,JET_TOOLCHAIN_ROOT:root,JET_STORE_DIR:join(out,'store')}});
  if (problem) throw Error(`candidate front ${problem.verdict}: ${problem.detail}`);
} else if (mode === 'audit') {
  const object = join(dir, 'fixture.o');
  const imports = command('nm', ['-u', '--format=posix', object]).stdout;
  writeFileSync(join(out, 'imports.nm'), imports);
  const allowed = new Set(readFileSync(join(root, 'Compiler/JetRuntime/imports.list'), 'utf8').split('\n').map(x => x.trim()).filter(x => x && !x.startsWith('#')));
  for (const line of imports.trim().split('\n').filter(Boolean)) {
    const symbol = line.split(/\s+/)[0];
    if (!allowed.has(symbol)) throw Error(`unledgered fixture import ${symbol}`);
  }
  writeFileSync(join(out, 'disassembly.txt'), command('objdump', ['-dr', object]).stdout);
} else if (mode === 'link') {
  command(nativeCCompiler, [join(root, 'Compiler/JetRuntime/Tests/test-suite-fixture.c'), join(dir, 'fixture.o'),
    '-lpthread', '-ldl', '-lm', '-o', join(out, 'test-suite-fixture')]);
} else {
  const result = spawnSync(join(out, 'test-suite-fixture'), [], { encoding: 'utf8', timeout: 300000 });
  writeFileSync(join(out, 'result.receipt.json'), JSON.stringify({ status: result.status, signal: result.signal,
    error: result.error?.message ?? null, stdout: result.stdout, stderr: result.stderr }, null, 2) + '\n');
  if (result.error || result.status !== 0 || result.stdout !== 'test suite: ok\n' || result.stderr !== '') throw Error(`test suite witness: ${result.error?.message ?? result.status}, stderr=${JSON.stringify(result.stderr)}`);
}
console.log(`test suite fixtures ${mode}: ok`);
