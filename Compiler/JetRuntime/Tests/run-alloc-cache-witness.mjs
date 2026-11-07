#!/usr/bin/env node
// Allocator per-thread block cache lane: the runtime sources plus
// AllocTests.jet (its alloc_test_live_blocks/alloc_test_cache_flush probes)
// compile to fixture.o; AllocCacheWitness.c runs its multi-thread stress mode.
// Usage: JET_FRONT_JETC0=<native CLI> node run-alloc-cache-witness.mjs OUT assemble|lower|audit|link|run
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import { faultReportObject } from '../../../Tools/packaging/fault-report-object.mjs';
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { candidateJetc0, runNativeLibrary, runtimeLifecycleList } from '../../JetBackend/Tests/jet-front.mjs';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, mode] = process.argv.slice(2);
if (!output || !['assemble', 'lower', 'audit', 'link', 'run'].includes(mode)) throw Error('usage: run-alloc-cache-witness.mjs OUT assemble|lower|audit|link|run');
const out = resolve(output), dir = join(out, 'unit');
function command(bin, args, options = {}) {
  const result = spawnSync(bin, args, { encoding: 'utf8', maxBuffer: 1 << 27, ...options });
  if (result.error || result.status !== 0) throw Error(`${bin}: ${result.status ?? result.signal}\n${result.error?.message ?? ''}\n${result.stdout ?? ''}${result.stderr ?? ''}`);
  return result;
}
if (mode === 'assemble') {
  mkdirSync(join(dir, 'Source'), { recursive: true });
  const paths = readFileSync(join(root, 'Compiler/JetRuntime/sources.list'), 'utf8').split('\n').map(x => x.trim()).filter(x => x && !x.startsWith('#'));
  paths.push('Compiler/JetRuntime/Tests/Fragments.fixture/AllocTests.jet');
  const unit = paths.map(path => readFileSync(join(root, path), 'utf8')).join('\n');
  writeFileSync(join(dir, 'Source/Fixture.jet'), unit);
  writeFileSync(join(dir, 'package.jet'), readFileSync(join(root, 'Compiler/JetRuntime/package.jet')));
  // The per-thread caches exist only on the unchecked heap. A system link
  // keeps every hook, so this fixture drops the checked heap's init row
  // (Alloc.jet jet_rt_alloc_check_init): the stress mode runs the allocator
  // a program built without `--checked-alloc` runs.
  const lifecycle = runtimeLifecycleList(unit).split('\n').filter(line => !line.startsWith('jet_rt_alloc_check_init ')).join('\n');
  writeFileSync(join(dir, 'lifecycle.list'), lifecycle);
  writeFileSync(join(dir, 'sources.receipt.json'), JSON.stringify({ paths }, null, 2) + '\n');
} else if (mode === 'lower') {
  const prefix = join(dir, 'fixture');
  const { problem } = runNativeLibrary({ jet: candidateJetc0(), file: join(dir, 'Source/Fixture.jet'), prefix, lifecycle: join(dir, 'lifecycle.list'), env: { ...process.env, JET_TOOLCHAIN_ROOT: root, JET_STORE_DIR: join(out, 'store') } });
  if (problem) throw Error(`candidate front ${problem.verdict}: ${problem.detail}`);
} else if (mode === 'audit') {
  const imports = command('nm', ['-u', '--format=posix', join(dir, 'fixture.o')]).stdout;
  const allowed = new Set(readFileSync(join(root, 'Compiler/JetRuntime/imports.list'), 'utf8').split('\n').map(x => x.trim()).filter(x => x && !x.startsWith('#')));
  for (const line of imports.trim().split('\n').filter(Boolean)) if (!allowed.has(line.split(/\s+/)[0])) throw Error(`unledgered fixture import ${line}`);
  const symbols = command('nm', ['-g', '--defined-only', '--format=posix', join(dir, 'fixture.o')]).stdout;
  for (const name of ['alloc_test_live_blocks', 'alloc_test_cache_flush', 'jet_rt_alloc', 'jet_rt_free']) if (symbols.split('\n').filter(line => line.startsWith(`${name} T `)).length !== 1) throw Error(`expected one strong Jet ${name}`);
} else if (mode === 'link') {
  // The fixture object imports jet_rt_fault_report, which Jet's linker writes
  // into each image; this platform link takes it from the shared report source.
  const faultReport = faultReportObject(mkdtempSync(join(out, 'fault-report-')));
  command(nativeCCompiler, ['-std=c11', '-O1', '-Wall', '-Wextra', '-Werror', join(root, 'Compiler/JetRuntime/Tests/AllocCacheWitness.c'), join(dir, 'fixture.o'), faultReport, '-lpthread', '-ldl', '-lm', '-o', join(out, 'alloc-cache-witness')]);
} else {
  const result = spawnSync(join(out, 'alloc-cache-witness'), ['stress'], { encoding: 'utf8', timeout: 600000 });
  writeFileSync(join(out, 'run.receipt.json'), JSON.stringify({ status: result.status, signal: result.signal, error: result.error?.message ?? null, stdout: result.stdout, stderr: result.stderr }, null, 2) + '\n');
  if (result.error || result.status !== 0 || result.signal || result.stderr || !/^alloc cache witness: ok \([^\n]*\)\n$/.test(result.stdout ?? '')) throw Error(`alloc cache witness: ${result.error?.message ?? result.status ?? result.signal}; stdout=${result.stdout}; stderr=${result.stderr}`);
  console.log(result.stdout.trim());
}
