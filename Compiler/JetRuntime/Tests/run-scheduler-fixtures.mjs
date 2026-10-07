#!/usr/bin/env node
// Real runtime source + Jet fixture callbacks; C only transports code pointers.
// The remaining Observe implementation is the freshly composed runtime pack,
// not a test observer, fake stop rail or alternate scheduler.
import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync, existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { candidateJetc0, frontReceipt } from '../../JetBackend/Tests/jet-front.mjs';
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
  paths.push('Compiler/JetRuntime/Tests/SchedulerTests.jet');
  const complete = paths.map(path => readFileSync(join(root, path), 'utf8')).join('\n');
  writeFileSync(join(dir, 'Source/Fixture.jet'), complete);
  writeFileSync(join(dir, 'package.jet'), readFileSync(join(root, 'Compiler/JetRuntime/package.jet')));
  const entries = readFileSync(join(root, 'Compiler/JetRuntime/exports.list'), 'utf8').split('\n').filter(x => x && !x.startsWith('#'));
  const fixture = readFileSync(join(root, 'Compiler/JetRuntime/Tests/SchedulerTests.jet'), 'utf8');
  for (const match of fixture.matchAll(/^#Export\(c\)\s+fn\s+(\w+)\(/gm)) entries.push(match[1]);
  writeFileSync(join(dir, 'exports.list'), entries.sort().join('\n') + '\n');
  writeFileSync(join(dir, 'sources.receipt.json'), JSON.stringify({ paths }, null, 2) + '\n');
} else if (mode === 'lower') {
  const prefix = join(dir, 'fixture');
  const receipt = join(dir, 'front.receipt');
  const result = spawnSync(candidateJetc0(), [], { cwd: root, encoding: 'utf8', maxBuffer: 1 << 27,
    timeout: Number(process.env.JET_FRONT_TIMEOUT_MS ?? 600000), env: { ...process.env,
      JET_BOOTSTRAP_MODE: 'factory', JET_BOOTSTRAP_FACTORY_TIER: 'aot', JET_BOOTSTRAP_SOURCE_ROOT: dir,
      JET_BOOTSTRAP_ENTRY: 'Source/Fixture.jet', JET_BOOTSTRAP_OUTPUT: join(dir, 'unused.output'), JET_BOOTSTRAP_RECEIPT: receipt,
      JET_FRONT_LIBRARY_OBJECT_OUTPUT: prefix, JET_FRONT_LIBRARY_EXPORTS: join(dir, 'exports.list'), JET_STORE_DIR: join(out, 'store'), RUST_MIN_STACK: '8388608' } });
  writeFileSync(join(dir, 'front.log'), `${result.stdout ?? ''}${result.stderr ?? ''}`);
  if (result.error || result.status !== 0) throw Error(`candidate front failed: ${result.error?.message ?? result.status}`);
  frontReceipt(receipt, { files: 1, package: 'jet_runtime' });
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
  if (!library || !existsSync(library)) throw Error('JET_RUNTIME_C_LIB must name the freshly built composed runtime; no historical/provider-only substitute');
  command(process.env.CC ?? 'cc', [join(root, 'Compiler/JetRuntime/Tests/scheduler-fixture.c'), join(dir, 'fixture.o'), library,
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
  writeFileSync(join(out, 'root-panic.receipt.json'), JSON.stringify({ status: panic.status, stdout: panic.stdout, stderr: panic.stderr }, null, 2) + '\n');
  // Compare the root process edge to the canonical L2 stderr golden supplied
  // by the same proof; the task-panic runs above require absolutely no stderr.
  const golden = process.env.JET_SCHEDULER_ROOT_PANIC_GOLDEN;
  if (!golden || !existsSync(golden)) failures.push('JET_SCHEDULER_ROOT_PANIC_GOLDEN must name the freshly rendered Rust-tier reference');
  else if (panic.error || panic.status !== 70 || panic.stdout !== '' || panic.stderr !== readFileSync(golden, 'utf8')) failures.push('root panic differs from the Rust-tier reference');
  if (failures.length) throw Error(failures.join('\n'));
}
