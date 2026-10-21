#!/usr/bin/env node
// One composed fixture: real World threads, all four MemSentry seed unit
// witnesses, a Source inline raw-read stop, and canonical ledger byte parity.
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync, existsSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { candidateJetc0 } from '../../JetBackend/Tests/jet-front.mjs';
import { runNativeLibrary } from '../../../Tools/agent/compiler-native-transport.mjs';
import { parseRuntimeExports, runtimeLifecycleList } from '../../../Tools/agent/runtime-route-abi.mjs';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, mode] = process.argv.slice(2);
if (mode === 'reference') {
  console.log("parked: Rust reference retired (owner #38/#59); Tower #4923");
  process.exit(0);
}
if (!output || !['assemble', 'lower', 'audit', 'link', 'run', 'render'].includes(mode)) throw Error('usage: run-world-sentry-fixtures.mjs OUT assemble|lower|audit|link|run|render');
const out = resolve(output), unit = join(out, 'unit');
const fixturePaths = ['WorldTests.jet', 'SentryTests.jet', 'SentryInlineTests.jet', 'TestScopeTests.jet', 'AllocationPolicyTests.jet'].map(name => 'Compiler/JetRuntime/Tests/Fragments.fixture/' + name);
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
  writeFileSync(join(unit, 'lifecycle.list'), runtimeLifecycleList(parseRuntimeExports(source, 'Source/Fixture.jet')));
  writeFileSync(join(out, 'sources.receipt.json'), JSON.stringify({ paths }, null, 2) + '\n');
} else if (mode === 'lower') {
  const prefix = join(unit, 'fixture');
  const { problem } = runNativeLibrary({ jet: candidateJetc0(), file: join(unit, 'Source/Fixture.jet'), prefix, lifecycle: join(unit, 'lifecycle.list'), timeout: Number(process.env.JET_FRONT_TIMEOUT_MS ?? 600000), env: { ...process.env, JET_TOOLCHAIN_ROOT: root, JET_STORE_DIR: join(out, 'store'), RUST_MIN_STACK: '8388608' } });
  if (problem) throw Error(`candidate front ${problem.verdict}: ${problem.detail}`);
} else if (mode === 'audit') {
  const object = join(unit, 'fixture.o');
  const imports = command('imports', 'nm', ['-u', '--format=posix', object]).stdout;
  const allowed = new Set(readFileSync(join(root, 'Compiler/JetRuntime/imports.list'), 'utf8').split('\n').map(x => x.trim()).filter(x => x && !x.startsWith('#')));
  for (const line of imports.trim().split('\n').filter(Boolean)) if (!allowed.has(line.split(/\s+/)[0])) throw Error(`unledgered import ${line}`);
  command('disassembly', 'objdump', ['-dr', object]);
} else if (mode === 'render') {
  mkdirSync(out, { recursive: true });
  const selected = 'Compiler/JetRuntime/Tests/link-render-cases.list';
  const destination = join(out, 'render-goldens');
  command('typed-render-goldens', process.execPath, [join(root, 'Compiler/JetBackend/Tests/run-goldens.mjs'), destination, '--front', 'jet', '--case-list', join(root, selected)], { timeout: 0 });
  const results = new Map(readFileSync(join(destination, 'results.tsv'), 'utf8').trim().split('\n').map(row => row.split('\t')).map(([stem, verdict]) => [stem, verdict]));
  for (const stem of readFileSync(join(root, selected), 'utf8').trim().split('\n')) {
    if (results.get(stem) !== 'pass') throw Error(`typed render ${stem}: ${results.get(stem) ?? 'missing result'}`);
  }
} else if (mode === 'link') {
  // Aggregated fixture.o already contains the complete Jet runtime.
  console.log("parked: Rust reference retired (owner #38/#59); Tower #4923");
  command('link', nativeCCompiler, [join(root, 'Compiler/JetRuntime/Tests/world-sentry-fixture.c'), join(unit, 'fixture.o'), '-lpthread', '-ldl', '-lm', '-o', join(out, 'world-sentry-fixture')]);
  command('logical-env-link', nativeCCompiler, [join(root, 'Compiler/JetRuntime/Tests/LogicalEnvironmentTests.c'), join(unit, 'fixture.o'), '-lpthread', '-ldl', '-lm', '-o', join(out, 'logical-env-fixture')]);
  command('link-native-link', nativeCCompiler, [join(root, 'Compiler/JetRuntime/Tests/LinkNativeTests.c'), join(unit, 'fixture.o'), '-lpthread', '-ldl', '-lm', '-o', join(out, 'link-native-fixture')]);
} else {
  const failures = [];
  // Raw-check predicates now observe faults too. Never write to checkout/.jet,
  // and each receipt uses new paths so append-only ledgers cannot hide duplicates.
  const ledgers = mkdtempSync(join(out, 'ledger-'));
  try {
    const result = command('runtime-witnesses', join(out, 'world-sentry-fixture'), [], { timeout: 300000,
      env: { ...process.env, JET_MEMORY_LEDGER: join(ledgers, 'raw-checks/ledger.jsonl') } });
    if (result.stdout !== '' || result.stderr !== '') throw Error('runtime witnesses changed empty output');
  } catch (error) { failures.push(error.message); }
  const referencePath = process.env.JET_SENTRY_LEDGER_REFERENCE ?? join(out, 'ledger-reference.jsonl');
  const nativePath = join(ledgers, 'return/ledger.jsonl'), stopPath = join(ledgers, 'stop/ledger.jsonl');
  console.log("parked: Rust reference retired (owner #38/#59); Tower #4923");
  for (const [name, binary, args, path] of [['native', join(out, 'world-sentry-fixture'), ['ledger'], nativePath]]) {
    try { command(name, binary, args, { timeout: 30000, env: { ...process.env, JET_MEMORY_LEDGER: path } }); }
    catch (error) { failures.push(error.message); }
  }
  const stopped = spawnSync(join(out, 'world-sentry-fixture'), ['ledger-stop'], { encoding: 'utf8', timeout: 30000,
    env: { ...process.env, JET_MEMORY_LEDGER: stopPath } });
  writeFileSync(join(out, 'ledger-stop.receipt.json'), JSON.stringify({ status: stopped.status, signal: stopped.signal,
    error: stopped.error?.message ?? null, stdout: stopped.stdout, stderr: stopped.stderr }, null, 2) + '\n');
  if (stopped.error || stopped.signal || stopped.status !== 70 || stopped.stdout !== '' || !stopped.stderr?.includes('R0802')) failures.push('fault+stop did not produce the canonical R0802 exit70');
  const exitMetadata = stopped.stderr?.split('\n').filter(line => line.startsWith('exit: ')) ?? [];
  if (exitMetadata.length !== 1 || exitMetadata[0] !== `exit: ${stopped.status}`) failures.push('sentry report exit metadata differs from the actual fault status');
  try {
    const native = readFileSync(nativePath);
    // Compare only saved oracle bytes, never execute a retired reference binary.
    if (existsSync(referencePath) && !readFileSync(referencePath).equals(native)) throw Error('ledger JSONL differs byte-for-byte from frozen reference');
    const stop = readFileSync(stopPath);
    for (const [name, bytes] of [['fault-return', native], ['fault-stop', stop]]) {
      if (bytes.toString('utf8').trimEnd().split('\n').length !== 1 || !bytes.length) throw Error(`${name} did not write exactly one ledger row`);
    }
    if (!stop.equals(native)) throw Error('fault-return and fault+stop ledger bytes differ');
    if ((statSync(nativePath).mode & 0o777) !== 0o600 || (statSync(dirname(nativePath)).mode & 0o777) !== 0o700) throw Error('ledger file/parent permissions differ');
  } catch (error) { failures.push(error.message); }
  if (failures.length) throw Error(failures.join('\n'));
}
