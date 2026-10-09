#!/usr/bin/env node
// Same full-object convention as run-scheduler-fixtures.mjs. Runtime and Jet
// callbacks are lowered together; the C witness only transports code pointers.
// Composition supplies the accepted raw-leaf library, never a Rust substitute.
import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync, existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { candidateJetc0, frontReceipt } from '../../JetBackend/Tests/jet-front.mjs';
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
  command(process.execPath, [join(root, 'Compiler/JetRuntime/Tests/check-runtime.mjs'), dir, '--assemble-only', '--test', 'TestSuite']);
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
  const prefix = join(dir, 'fixture'), receipt = join(dir, 'front.receipt');
  const result = spawnSync(candidateJetc0(), [], { cwd: root, encoding: 'utf8', maxBuffer: 1 << 27,
    timeout: Number(process.env.JET_FRONT_TIMEOUT_MS ?? 600000), env: { ...process.env,
      JET_BOOTSTRAP_MODE: 'factory', JET_BOOTSTRAP_FACTORY_TIER: 'aot', JET_BOOTSTRAP_SOURCE_ROOT: dir,
      JET_BOOTSTRAP_ENTRY: 'Source/Fixture.jet', JET_BOOTSTRAP_OUTPUT: join(dir, 'unused.output'), JET_BOOTSTRAP_RECEIPT: receipt,
      JET_FRONT_LIBRARY_OBJECT_OUTPUT: prefix, JET_FRONT_LIBRARY_EXPORTS: join(dir, 'exports.list'), JET_STORE_DIR: join(out, 'store'), RUST_MIN_STACK: '8388608' } });
  writeFileSync(join(dir, 'front.log'), `${result.stdout ?? ''}${result.stderr ?? ''}`);
  const problem = frontReceipt(existsSync(receipt) ? readFileSync(receipt, 'utf8') : '', result);
  if (problem) throw Error(`candidate front ${problem.verdict}: ${problem.detail}`);
  if (existsSync(prefix + '.issues')) throw Error(readFileSync(prefix + '.issues', 'utf8'));
  if (!existsSync(prefix + '.o')) throw Error('candidate did not emit the real runtime fixture object');
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
  const library = process.env.JET_RUNTIME_C_LIB;
  if (!library || !existsSync(library)) throw Error('JET_RUNTIME_C_LIB must name the freshly composed accepted raw-leaf library, not a Rust runtime');
  const symbols = command('nm', ['--defined-only', '--format=posix', library]).stdout;
  if (/(?:^|\n)(?:_R|_ZN|__rust|rust_eh_personality)/u.test(symbols)) throw Error('JET_RUNTIME_C_LIB contains Rust symbols; a raw-leaf-only native library is required');
  command(process.env.CC ?? 'cc', [join(root, 'Compiler/JetRuntime/Tests/test-suite-fixture.c'), join(dir, 'fixture.o'), library,
    '-lpthread', '-ldl', '-lm', '-o', join(out, 'test-suite-fixture')]);
} else {
  const result = spawnSync(join(out, 'test-suite-fixture'), [], { encoding: 'utf8', timeout: 300000 });
  writeFileSync(join(out, 'result.receipt.json'), JSON.stringify({ status: result.status, signal: result.signal,
    error: result.error?.message ?? null, stdout: result.stdout, stderr: result.stderr }, null, 2) + '\n');
  if (result.error || result.status !== 0 || result.stdout !== 'test suite: ok\n' || result.stderr !== '') throw Error(`test suite witness: ${result.error?.message ?? result.status}, stderr=${JSON.stringify(result.stderr)}`);
}
console.log(`test suite fixtures ${mode}: ok`);
