#!/usr/bin/env node
// Full Jet source/object lane: fixture.o supplies the runtime, not another pack.
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { candidateJetc0 } from '../../JetBackend/Tests/jet-front.mjs';
import { runNativeLibrary } from '../../../Tools/agent/compiler-native-transport.mjs';
import { parseRuntimeExports, runtimeLifecycleList } from '../../../Tools/agent/runtime-route-abi.mjs';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, mode] = process.argv.slice(2);
if (!output || !['assemble', 'lower', 'audit', 'link', 'run'].includes(mode)) throw Error('usage: run-parallel-fixtures.mjs OUT assemble|lower|audit|link|run');
const out = resolve(output), dir = join(out, 'unit');
function command(bin, args, options = {}) {
  const result = spawnSync(bin, args, { encoding: 'utf8', maxBuffer: 1 << 27, ...options });
  if (result.error || result.status !== 0) throw Error(`${bin}: ${result.status ?? result.signal}\n${result.error?.message ?? ''}\n${result.stdout ?? ''}${result.stderr ?? ''}`);
  return result;
}
if (mode === 'assemble') {
  mkdirSync(join(dir, 'Source'), { recursive: true });
  const paths = readFileSync(join(root, 'Compiler/JetRuntime/sources.list'), 'utf8').split('\n').map(x => x.trim()).filter(x => x && !x.startsWith('#'));
  paths.push('Compiler/JetRuntime/Tests/ParallelTests.jet');
  const unit = paths.map(path => readFileSync(join(root, path), 'utf8')).join('\n');
  writeFileSync(join(dir, 'Source/Fixture.jet'), unit);
  writeFileSync(join(dir, 'package.jet'), readFileSync(join(root, 'Compiler/JetRuntime/package.jet')));
  writeFileSync(join(dir, 'lifecycle.list'), runtimeLifecycleList(parseRuntimeExports(unit, 'Source/Fixture.jet')));
  writeFileSync(join(dir, 'sources.receipt.json'), JSON.stringify({ paths }, null, 2) + '\n');
} else if (mode === 'lower') {
  const prefix = join(dir, 'fixture');
  const { problem } = runNativeLibrary({ jet: candidateJetc0(), file: join(dir, 'Source/Fixture.jet'), prefix, lifecycle: join(dir, 'lifecycle.list'), env: { ...process.env, JET_TOOLCHAIN_ROOT: root, JET_STORE_DIR: join(out, 'store') } });
  if (problem) throw Error(`candidate front ${problem.verdict}: ${problem.detail}`);
} else if (mode === 'audit') {
  const imports = command('nm', ['-u', '--format=posix', join(dir, 'fixture.o')]).stdout;
  const allowed = new Set(readFileSync(join(root, 'Compiler/JetRuntime/imports.list'), 'utf8').split('\n').map(x => x.trim()).filter(x => x && !x.startsWith('#')));
  writeFileSync(join(out, 'imports.nm'), imports);
  for (const line of imports.trim().split('\n').filter(Boolean)) if (!allowed.has(line.split(/\s+/)[0])) throw Error(`unledgered fixture import ${line}`);
  const symbols = command('nm', ['-g', '--defined-only', '--format=posix', join(dir, 'fixture.o')]).stdout;
  for (const name of ['jet_rt_native_para_walk', 'jet_rt_native_para_worker']) if (symbols.split('\n').filter(line => line.startsWith(`${name} T `)).length !== 1) throw Error(`expected one strong Jet ${name}`);
  writeFileSync(join(out, 'symbols.nm'), symbols);
} else if (mode === 'link') {
  command(nativeCCompiler, [join(root, 'Compiler/JetRuntime/Tests/parallel-fixture.c'), join(dir, 'fixture.o'), '-lpthread', '-ldl', '-lm', '-o', join(out, 'parallel-fixture')]);
} else {
  const result = spawnSync(join(out, 'parallel-fixture'), [], { encoding: 'utf8', timeout: 300000 });
  writeFileSync(join(out, 'run.receipt.json'), JSON.stringify({ status: result.status, signal: result.signal, error: result.error?.message ?? null, stdout: result.stdout, stderr: result.stderr }, null, 2) + '\n');
  if (result.error || result.status !== 0 || result.signal || result.stdout || result.stderr) throw Error(`parallel fixture: ${result.error?.message ?? result.status}; stderr=${result.stderr}`);
  console.log('parallel walk: empty, serial limit, tail chunk, joined failures and allocator-class balance passed');
}
