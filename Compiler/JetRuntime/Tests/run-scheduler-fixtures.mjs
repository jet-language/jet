#!/usr/bin/env node
// Real runtime source + Jet fixture callbacks in one library object.
// C only transports code pointers; no Rust provider or substitute scheduler.
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { candidateJetc0 } from '../../JetBackend/Tests/jet-front.mjs';
import { runNativeLibrary } from '../../../Tools/agent/compiler-native-transport.mjs';
import { parseRuntimeExports, runtimeLifecycleList } from '../../../Tools/agent/runtime-route-abi.mjs';
import { trapLiteral, schedulerLiteral } from './report-literals.mjs';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, mode] = process.argv.slice(2);
if (!output || !['assemble', 'lower', 'audit', 'link', 'run'].includes(mode)) throw Error('usage: run-scheduler-fixtures.mjs OUT assemble|lower|audit|link|run');
const out = resolve(output);
const dir = join(out, 'unit');
function command(bin, args, options = {}) {
  const result = spawnSync(bin, args, { encoding: 'utf8', maxBuffer: 1 << 27, ...options });
  if (result.error || result.status !== 0) throw Error(`${bin}: ${result.status ?? result.signal}\n${result.error?.message ?? ''}\n${result.stdout ?? ''}${result.stderr ?? ''}`);
  return result;
}
if (mode === 'assemble') {
  mkdirSync(join(dir, 'Source'), { recursive: true });
  const paths = readFileSync(join(root, 'Compiler/JetRuntime/sources.list'), 'utf8').split('\n').map(x => x.trim()).filter(x => x && !x.startsWith('#'));
  paths.push('Compiler/JetRuntime/Tests/Fragments.fixture/SchedulerTests.jet');
  const complete = paths.map(path => readFileSync(join(root, path), 'utf8')).join('\n');
  writeFileSync(join(dir, 'Source/Fixture.jet'), complete);
  writeFileSync(join(dir, 'package.jet'), readFileSync(join(root, 'Compiler/JetRuntime/package.jet')));
  writeFileSync(join(dir, 'lifecycle.list'), runtimeLifecycleList(parseRuntimeExports(complete, 'Source/Fixture.jet')));
  writeFileSync(join(dir, 'sources.receipt.json'), JSON.stringify({ paths }, null, 2) + '\n');
} else if (mode === 'lower') {
  const prefix = join(dir, 'fixture');
  const {problem} = runNativeLibrary({jet:candidateJetc0(),file:join(dir,'Source/Fixture.jet'),prefix,lifecycle:join(dir,'lifecycle.list'),env:{...process.env,JET_TOOLCHAIN_ROOT:root,JET_STORE_DIR:join(out,'store'),RUST_MIN_STACK:'8388608'}});
  if (problem) throw Error(problem.detail);
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
  command(nativeCCompiler, [join(root, 'Compiler/JetRuntime/Tests/scheduler-fixture.c'), join(dir, 'fixture.o'),
    '-lpthread', '-ldl', '-lm', '-o', join(out, 'scheduler-fixture')]);
} else {
  const failures = [];
  for (const threads of ['1', '2', '8']) {
    const result = spawnSync(join(out, 'scheduler-fixture'), [], { encoding: 'utf8', timeout: 300000,
      env: { ...process.env, JET_SCHEDULER_THREADS: threads } });
    writeFileSync(join(out, `threads-${threads}.receipt.json`), JSON.stringify({ status: result.status, signal: result.signal,
      error: result.error?.message ?? null, stdout: result.stdout, stderr: result.stderr }, null, 2) + '\n');
    if (result.error || result.status !== 0 || result.stdout !== '' || result.stderr !== '') failures.push(`threads=${threads}: ${result.error?.message ?? result.status}, stderr=${JSON.stringify(result.stderr)}`);
  }
  const panic = spawnSync(join(out, 'scheduler-fixture'), ['root-panic'], { encoding: 'utf8', timeout: 30000 });
  writeFileSync(join(out, 'root-panic.receipt.json'), JSON.stringify({ status: panic.status, signal: panic.signal,
    error: panic.error?.message ?? null, stdout: panic.stdout, stderr: panic.stderr }, null, 2) + '\n');
  // The committed report literals produce the complete golden without a
  // Rust-tier dependency; task-local panics above must still emit no stderr.
  const golden = Buffer.concat([trapLiteral('trap_msg_header_panic'),
    schedulerLiteral('scheduler_msg_cancelled'), trapLiteral('trap_msg_prelude_end'), trapLiteral('trap_footer_e3001')]);
  writeFileSync(join(out, 'root-panic.golden.stderr'), golden);
  if (panic.error || panic.signal !== null || panic.status !== 70 || panic.stdout !== '' ||
      !Buffer.from(panic.stderr ?? '').equals(golden)) failures.push('root panic differs from the canonical process-edge golden');
  if (failures.length) throw Error(failures.join('\n'));
}
