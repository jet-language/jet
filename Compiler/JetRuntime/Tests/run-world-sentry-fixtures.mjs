#!/usr/bin/env node
// One composed fixture: real World threads, all four MemSentry seed unit
// witnesses, a Source inline raw-read stop, and canonical ledger byte parity.
import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync, existsSync, statSync, openSync, closeSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { candidateJetc0, frontReceipt } from '../../JetBackend/Tests/jet-front.mjs';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, mode] = process.argv.slice(2);
if (!output || !['assemble', 'lower', 'audit', 'link', 'run'].includes(mode)) throw Error('usage: run-world-sentry-fixtures.mjs OUT assemble|lower|audit|link|run');
const out = resolve(output), unit = join(out, 'unit');
const fixturePaths = ['WorldTests.jet', 'SentryTests.jet', 'SentryInlineTests.jet', 'TestScopeTests.jet', 'TrapReportTests.jet', 'HostFaultTests.jet'].map(name => 'Compiler/JetRuntime/Tests/' + name);
function command(name, binary, args, options = {}) {
  const result = spawnSync(binary, args, { cwd: root, encoding: 'utf8', maxBuffer: 1 << 27, ...options });
  writeFileSync(join(out, name + '.receipt.json'), JSON.stringify({ status: result.status, signal: result.signal, error: result.error?.message ?? null, stdout: result.stdout, stderr: result.stderr }, null, 2) + '\n');
  if (result.error || result.status !== 0) throw Error(`${name}: ${result.error?.message ?? result.status}\n${result.stdout ?? ''}${result.stderr ?? ''}`);
  return result;
}
if (mode === 'assemble') {
  mkdirSync(join(unit, 'Source'), { recursive: true });
  const paths = readFileSync(join(root, 'Compiler/JetRuntime/sources.list'), 'utf8').split('\n').map(x => x.trim()).filter(x => x && !x.startsWith('#')).concat(fixturePaths);
  const source = paths.map(path => readFileSync(join(root, path), 'utf8')).join('\n');
  writeFileSync(join(unit, 'Source/Fixture.jet'), source);
  writeFileSync(join(unit, 'package.jet'), readFileSync(join(root, 'Compiler/JetRuntime/package.jet')));
  const entries = readFileSync(join(root, 'Compiler/JetRuntime/exports.list'), 'utf8').split('\n').filter(x => x && !x.startsWith('#'));
  // Adjacent Source markers use the canonical bracket list.
  for (const path of fixturePaths) for (const match of readFileSync(join(root, path), 'utf8').matchAll(/#(?:Export\(c\)|\[Export\(c\),\s*Unsafe\([^\n]*\)\])\s+fn\s+(\w+)\(/g)) entries.push(match[1]);
  writeFileSync(join(unit, 'exports.list'), entries.sort().join('\n') + '\n');
  writeFileSync(join(out, 'sources.receipt.json'), JSON.stringify({ paths }, null, 2) + '\n');
} else if (mode === 'lower') {
  const prefix = join(unit, 'fixture'), receipt = join(unit, 'front.receipt');
  const result = command('front', candidateJetc0(), [], { timeout: Number(process.env.JET_FRONT_TIMEOUT_MS ?? 600000), env: { ...process.env,
    JET_BOOTSTRAP_MODE: 'factory', JET_BOOTSTRAP_FACTORY_TIER: 'aot', JET_BOOTSTRAP_SOURCE_ROOT: unit,
    JET_BOOTSTRAP_ENTRY: 'Source/Fixture.jet', JET_BOOTSTRAP_OUTPUT: join(unit, 'unused.output'), JET_BOOTSTRAP_RECEIPT: receipt,
    JET_FRONT_LIBRARY_OBJECT_OUTPUT: prefix, JET_FRONT_LIBRARY_EXPORTS: join(unit, 'exports.list'), JET_STORE_DIR: join(out, 'store'), RUST_MIN_STACK: '8388608' } });
  const problem = frontReceipt(existsSync(receipt) ? readFileSync(receipt, 'utf8') : '', result);
  if (problem) throw Error(`candidate front ${problem.verdict}: ${problem.detail}`);
  if (existsSync(prefix + '.issues')) throw Error(readFileSync(prefix + '.issues', 'utf8'));
  if (!existsSync(prefix + '.o')) throw Error('candidate did not emit the real runtime fixture object');
} else if (mode === 'audit') {
  const object = join(unit, 'fixture.o');
  const imports = command('imports', 'nm', ['-u', '--format=posix', object]).stdout;
  const allowed = new Set(readFileSync(join(root, 'Compiler/JetRuntime/imports.list'), 'utf8').split('\n').map(x => x.trim()).filter(x => x && !x.startsWith('#')));
  for (const line of imports.trim().split('\n').filter(Boolean)) if (!allowed.has(line.split(/\s+/)[0])) throw Error(`unledgered import ${line}`);
  command('disassembly', 'objdump', ['-dr', object]);
} else if (mode === 'link') {
  const library = process.env.JET_RUNTIME_C_LIB, rlib = process.env.JET_RUNTIME_NATIVE_RLIB;
  if (!library || !existsSync(library) || !rlib || !existsSync(rlib)) throw Error('fresh composed JET_RUNTIME_C_LIB and JET_RUNTIME_NATIVE_RLIB required');
  command('link', process.env.CC ?? 'cc', [join(root, 'Compiler/JetRuntime/Tests/world-sentry-fixture.c'), join(unit, 'fixture.o'), library, '-lpthread', '-ldl', '-lm', '-o', join(out, 'world-sentry-fixture')]);
  command('logical-env-link', process.env.CC ?? 'cc', [join(root, 'Compiler/JetRuntime/Tests/LogicalEnvironmentTests.c'), join(unit, 'fixture.o'), library, '-lpthread', '-ldl', '-lm', '-o', join(out, 'logical-env-fixture')]);
  command('link-native-link', process.env.CC ?? 'cc', [join(root, 'Compiler/JetRuntime/Tests/LinkNativeTests.c'), join(unit, 'fixture.o'), library, '-lpthread', '-ldl', '-lm', '-o', join(out, 'link-native-fixture')]);
  command('reference-link', 'rustc', ['--edition', '2021', join(root, 'Compiler/JetRuntime/Tests/sentry-ledger-reference.rs'), '--extern', `jet_runtime=${rlib}`, '-C', `link-arg=${join(unit, 'fixture.o')}`, '-o', join(out, 'ledger-reference')]);
} else {
  const failures = [];
  try {
    const result = command('runtime-witnesses', join(out, 'world-sentry-fixture'), [], { timeout: 300000 });
    if (result.stdout !== '' || result.stderr !== '') throw Error('runtime witnesses changed empty output');
  } catch (error) { failures.push(error.message); }
  for (const mode of ['normal', 'empty']) {
    try {
      const result = command(`logical-env-witnesses-${mode}`, join(out, 'logical-env-fixture'), mode === 'empty' ? ['empty'] : [], { timeout: 30000 });
      if (result.stdout !== '' || result.stderr !== '') throw Error(`logical environment ${mode} witnesses changed empty output`);
    } catch (error) { failures.push(error.message); }
  }
  try {
    const result = command('link-native-goldens', join(out, 'link-native-fixture'), [], { timeout: 30000 });
    const expected = 'shared read=7 edit=8 read=8\nselect wait=0 value=42 probe=-1\natexit=21 drops=2\nprogress=4,5 none=1\n';
    if (result.stdout !== expected || result.stderr !== '') throw Error('existing native provider link goldens changed bytes');
  } catch (error) { failures.push(error.message); }
  // Exercise both sides of the real stop boundary. Nonzero 70 is expected,
  // so retain both raw results rather than treating the first stop as failure.
  for (let mode = 0; mode < 4; mode++) {
    const pair = [mode, mode + 4].map(number => spawnSync(join(out, 'world-sentry-fixture'), ['trap-report', String(number)], { cwd: root, encoding: 'utf8', timeout: 30000 }));
    writeFileSync(join(out, `trap-report-${mode}.receipt.json`), JSON.stringify(pair, null, 2) + '\n');
    if (pair.some(result => result.error || result.signal || result.status !== 70 || result.stdout !== '') || pair[0].stderr !== pair[1].stderr || !pair[0].stderr.startsWith('Stop [E3001]: ')) failures.push(`trap report ${mode}: process/owned bytes or status differ`);
  }
  // A shared stdout/stderr descriptor proves chronology, not just two final
  // buffers containing the expected bytes in an unobservable order.
  for (const captured of [false, true]) {
    const path = join(out, `trap-buffered-${captured}.bytes`);
    const descriptor = openSync(path, 'w');
    const result = spawnSync(join(out, 'world-sentry-fixture'), ['trap-buffered', captured ? '4' : '0'], { cwd: root, stdio: ['ignore', descriptor, descriptor], timeout: 30000 });
    closeSync(descriptor);
    writeFileSync(join(out, `trap-buffered-${captured}.receipt.json`), JSON.stringify({ status: result.status, signal: result.signal, error: result.error?.message ?? null }, null, 2) + '\n');
    const bytes = readFileSync(path, 'utf8');
    if (result.error || result.signal || result.status !== 70 || !bytes.startsWith('buffered stdout\nStop [E3001]: ')) failures.push(`buffered stdout order (${captured ? 'captured' : 'process'}) differs`);
  }
  for (const [name, bytes] of [
    ['host-fault', 'A\n'],
    ['unreachable', 'internal error: entered unreachable code: A\n'],
  ]) {
    const result = spawnSync(join(out, 'world-sentry-fixture'), [name], { cwd: root, encoding: 'utf8', timeout: 30000 });
    writeFileSync(join(out, `${name}.receipt.json`), JSON.stringify(result, null, 2) + '\n');
    if (result.error || result.signal || result.status !== 101 || result.stdout || result.stderr !== bytes) failures.push(`${name}: host-fault class or canonical unreachable bytes changed`);
  }
  const referencePath = join(out, 'reference-ledger/ledger.jsonl'), nativePath = join(out, 'native-ledger/ledger.jsonl');
  for (const [name, binary, args, path] of [['reference', join(out, 'ledger-reference'), [], referencePath], ['native', join(out, 'world-sentry-fixture'), ['ledger'], nativePath]]) {
    try { command(name, binary, args, { timeout: 30000, env: { ...process.env, JET_MEMORY_LEDGER: path } }); }
    catch (error) { failures.push(error.message); }
  }
  try {
    const reference = readFileSync(referencePath), native = readFileSync(nativePath);
    if (!reference.equals(native)) throw Error('ledger JSONL differs byte-for-byte from canonical Rust writer');
    if ((statSync(nativePath).mode & 0o777) !== 0o600 || (statSync(dirname(nativePath)).mode & 0o777) !== 0o700) throw Error('ledger file/parent permissions differ');
  } catch (error) { failures.push(error.message); }
  if (failures.length) throw Error(failures.join('\n'));
}
