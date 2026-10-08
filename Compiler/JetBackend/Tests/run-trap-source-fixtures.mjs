#!/usr/bin/env node
// Registered-defer acceptance, compiled through the actual Source producer.
// Native programs use jet_codegen_lower_native_program; library objects use
// the unchanged plain producer. C transports entries into the real Task API.
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { candidateJetc0, frontReceipt } from './jet-front.mjs';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, mode] = process.argv.slice(2);
if (!output || !['assemble', 'lower', 'link', 'run'].includes(mode)) throw Error('usage: run-trap-source-fixtures.mjs OUT assemble|lower|link|run');
const out = resolve(output);
const cases = [
  { name: 'checked-defer', example: 'release_defer_panic', native: true, task: true, status: 0 },
  { name: 'plain-defer', example: 'release_defer_panic', native: false, task: true, status: 70 },
  { name: 'plain-defer-root', example: 'release_defer_panic', native: false, task: false, status: 70 },
  { name: 'plain-byte-identity', example: 'release_defer_lifo', native: false, task: false, status: 0 },
  { name: 'cleanup-panic', example: 'release_defer_panic', native: true, task: true, status: 70, cleanupPanic: true },
  { name: 'cleanup-cancel', example: 'release_defer_lifo', native: true, task: true, status: 0, cleanupCancel: true },
];
function command(name, binary, args, options = {}, expected = 0) {
  const result = spawnSync(binary, args, { cwd: root, encoding: 'utf8', maxBuffer: 1 << 27, ...options });
  writeFileSync(join(out, name + '.receipt.json'), JSON.stringify({ status: result.status, signal: result.signal, error: result.error?.message ?? null, stdout: result.stdout, stderr: result.stderr }, null, 2) + '\n');
  if (result.error || result.signal || result.status !== expected) throw Error(`${name}: ${result.error?.message ?? result.status}\n${result.stdout ?? ''}${result.stderr ?? ''}`);
  return result;
}
if (mode === 'assemble') {
  mkdirSync(out, { recursive: true });
  for (const c of cases) {
    const unit = join(out, c.name);
    mkdirSync(join(unit, 'Source'), { recursive: true });
    const sourcePath = `Examples/features/memory/${c.example}.jet`;
    let source = readFileSync(join(root, sourcePath), 'utf8');
    if (!c.native) source = source.replace('fn run()', '#Export(c) fn run()');
    if (c.cleanupPanic) source = source.replace('fn close(^self) { print("close {self.name}") }', 'fn close(^self) {\n    print("close {self.name}")\n    if self.name == "second" -> panic("close stop")\n}');
    if (c.cleanupCancel) {
      source = 'use core.tasks as tasks\n' + source.replace('fn run()', 'fn scoped()').replace('fn close(^self) { print("close {self.name}") }', 'fn close(^self) {\n    print("close {self.name}")\n    tasks.yield_now()\n}')
        + '\nfn run() {\n    scoped()\n    tasks.yield_now()\n    panic("cancellation was not delivered")\n}\n';
    }
    writeFileSync(join(unit, 'Source/Fixture.jet'), source);
    writeFileSync(join(unit, 'package.jet'), 'name: "trap_source_fixture"\nversion: "0.0.1"\nedition: "2028"\n');
    if (!c.native) writeFileSync(join(unit, 'exports.list'), 'run\n');
    writeFileSync(join(unit, 'source.receipt.json'), JSON.stringify({ sourcePath, native: c.native, task: c.task, cleanupPanic: c.cleanupPanic ?? false }, null, 2) + '\n');
  }
} else if (mode === 'lower') {
  const pack = process.env.JET_RUNTIME_PACK;
  if (!pack || !existsSync(pack)) throw Error('fresh composed JET_RUNTIME_PACK required');
  const frontEnv = { ...process.env };
  for (const key of ['JET_FRONT_NATIVE_OUTPUT', 'JET_FRONT_LIBRARY_OBJECT_OUTPUT', 'JET_FRONT_LIBRARY_EXPORTS']) delete frontEnv[key];
  for (const c of cases) {
    const unit = join(out, c.name), prefix = join(unit, 'fixture'), receipt = join(unit, 'front.receipt');
    const selectedOutput = c.native ? { JET_FRONT_NATIVE_OUTPUT: prefix } : { JET_FRONT_LIBRARY_OBJECT_OUTPUT: prefix, JET_FRONT_LIBRARY_EXPORTS: join(unit, 'exports.list') };
    const result = command(c.name + '-front', candidateJetc0(), [], { timeout: Number(process.env.JET_FRONT_TIMEOUT_MS ?? 600000), env: { ...frontEnv,
      JET_BOOTSTRAP_MODE: 'factory', JET_BOOTSTRAP_FACTORY_TIER: 'aot', JET_BOOTSTRAP_SOURCE_ROOT: unit,
      JET_BOOTSTRAP_ENTRY: 'Source/Fixture.jet', JET_BOOTSTRAP_OUTPUT: join(unit, 'unused.output'), JET_BOOTSTRAP_RECEIPT: receipt,
      ...selectedOutput,
      JET_RUNTIME_PACK: pack, JET_STORE_DIR: join(out, 'store'), RUST_MIN_STACK: '8388608' } });
    const problem = frontReceipt(existsSync(receipt) ? readFileSync(receipt, 'utf8') : '', result);
    if (problem) throw Error(`${c.name}: ${problem.verdict}: ${problem.detail}`);
    for (const suffix of ['.issues', '.pack-issues']) if (existsSync(prefix + suffix)) throw Error(readFileSync(prefix + suffix, 'utf8'));
    if (!existsSync(prefix + '.o')) throw Error(`${c.name}: no actual compiled Source object`);
  }
} else if (mode === 'link') {
  const library = process.env.JET_RUNTIME_C_LIB;
  if (!library || !existsSync(library)) throw Error('fresh composed JET_RUNTIME_C_LIB required');
  const host = join(root, 'Compiler/JetBackend/Tests/check-trap-task.c');
  for (const c of cases) {
    const unit = join(out, c.name);
    const args = [join(unit, 'fixture.o'), host, ...(c.native ? [] : ['-DJET_FX_LIBRARY_ENTRY']), ...(c.task ? ['-Wl,--wrap=jet_rt_main'] : ['-DJET_FX_PLAIN_ROOT']), ...(c.cleanupCancel ? ['-DJET_FX_CANCEL_DURING_CLEANUP', '-Wl,--wrap=jet_rt_cleanup_enter'] : []), library, '-lpthread', '-ldl', '-lm', '-o', join(unit, 'hosted')];
    command(c.name + '-link', process.env.CC ?? 'cc', args);
  }
} else {
  const results = new Map();
  for (const c of cases) {
    const result = command(c.name + '-run', join(out, c.name, 'hosted'), [], { timeout: 30000 }, c.status);
    const base = readFileSync(join(root, `Examples/features/expected/memory/${c.example}.out`), 'utf8');
    const expected = c.cleanupPanic ? 'body\nclose second\n' : base + (c.native && c.task ? (c.cleanupCancel ? 'task cancelled; process continued\n' : 'task published; process continued\n') : '');
    if (result.stdout !== expected) throw Error(`${c.name}: registered closes did not run exactly once in LIFO order: ${JSON.stringify(result.stdout)} != ${JSON.stringify(expected)}`);
    if (c.status === 0 && result.stderr !== '') throw Error(`${c.name}: unexpected stderr`);
    if (c.status === 70 && !result.stderr.startsWith('Stop [E3001]: ')) throw Error(`${c.name}: no actual process-fatal report`);
    if (c.cleanupPanic && !result.stderr.includes('panic: close stop')) throw Error('cleanup-panic: original Stop was wrongly returned instead of fatal cleanup panic');
    results.set(c.name, result);
  }
  if (results.get('plain-defer').stderr !== results.get('plain-defer-root').stderr) throw Error('plain task/root fatal report bytes differ');
  console.log('registered defer Source fixtures: CHECK OK');
}
