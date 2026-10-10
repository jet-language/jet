#!/usr/bin/env node
// Focused in-process checked producer proof. Build the assembled unit through
// the composed compiler-unit job, then run produce/link/run with that binary.
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, mode, aggregate] = process.argv.slice(2);
if (!output || !['assemble', 'produce', 'link', 'run'].includes(mode)) throw Error('usage: run-expect-producer.mjs OUT assemble COMPILER.jet|produce UNIT_BINARY|link|run');
const out = resolve(output), prefix = join(out, 'producer');
function command(name, binary, args, options = {}) {
  const run = spawnSync(binary, args, { cwd: root, encoding: 'utf8', maxBuffer: 1 << 27, ...options });
  writeFileSync(join(out, `${name}.receipt.json`), JSON.stringify({ status: run.status, signal: run.signal, error: run.error?.message ?? null, stdout: run.stdout, stderr: run.stderr }, null, 2) + '\n');
  if (run.error || run.signal || run.status !== 0) throw Error(`${name}: ${run.error?.message ?? run.status}\n${run.stdout ?? ''}${run.stderr ?? ''}`);
  return run;
}
if (mode === 'assemble') {
  if (!aggregate) throw Error('canonical assembled compiler aggregate required');
  mkdirSync(join(out, 'fixture/Source'), { recursive: true });
  const source = readFileSync(resolve(aggregate), 'utf8');
  const dispatch = 'else -> jet_driver_compile_mir(^request)';
  if (source.split(dispatch).length !== 2) throw Error('expected exactly one canonical CLI MIR request dispatch');
  if (/^\s*(?:pub )?fn run\(/m.test(source)) throw Error('compiler aggregate already has a run entry');
  const fixture = readFileSync(join(root, 'Compiler/JetBackend/Tests/ExpectProducer.jet'), 'utf8').replace(/^use jet_\w+\.\[[^\]]*\]\s*$/gm, '');
  writeFileSync(join(out, 'unit.jet'), source.replace(dispatch, 'else -> fx_expect_producer_compile(^request)') + '\n' + fixture);
  writeFileSync(join(out, 'fixture/package.jet'), 'name: "native_expect_producer"\nversion: "0.0.1"\nedition: "2028"\n');
  writeFileSync(join(out, 'fixture/Source/Test.jet'), 'struct Resource { name: String }\nimpl Resource.Close { fn close(^self) { print("close {self.name}") } }\n#Test("native expectation producer") {\n    .expect_fail(E3001, message: "producer boom") {\n        own := Resource{name: "own"}\n        defer close(^own)\n        panic("producer boom")\n    }\n    print("producer continued")\n}\n');
  console.log('native expectation producer unit assembled');
} else if (mode === 'produce') {
  if (!aggregate || !existsSync(resolve(aggregate))) throw Error('built composed producer UNIT_BINARY required');
  const run = command('produce', resolve(aggregate), [], { timeout: Number(process.env.JET_FRONT_TIMEOUT_MS ?? 600000), env: { ...process.env, JET_EXPECT_PRODUCER_SOURCE: join(out, 'fixture/Source/Test.jet'), JET_EXPECT_PRODUCER_OUTPUT: prefix } });
  if (run.stdout !== 'native expectation producer: CHECK OK\n' || run.stderr !== '') throw Error('producer structural assertion receipt differs');
  if (!existsSync(prefix + '.o') || !existsSync(prefix + '.mir')) throw Error('producer did not emit actual checked MIR and native object');
} else if (mode === 'link') {
  const library = process.env.JET_RUNTIME_C_LIB;
  if (!library || !existsSync(library)) throw Error('fresh composed JET_RUNTIME_C_LIB required');
  command('link', nativeCCompiler, [prefix + '.o', join(root, 'Compiler/JetBackend/Tests/check-trap-task.c'), '-DJET_FX_TASK_PASS', '-Wl,--wrap=jet_rt_main', library, '-lpthread', '-ldl', '-lm', '-o', prefix + '.hosted']);
} else {
  const run = command('run', prefix + '.hosted', [], { timeout: 30000 });
  if (run.stdout !== 'close own\nproducer continued\nexpectation task passed\n' || run.stderr !== '') throw Error('actual producer expectation did not catch and close exactly once');
  console.log('compiled native expectation producer: CHECK OK');
}
