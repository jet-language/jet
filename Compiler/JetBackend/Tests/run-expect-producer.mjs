#!/usr/bin/env node
// Focused in-process checked producer proof of the native `.expect_fail` lowering.
//   run-expect-producer.mjs OUT assemble
//     copies the compiler packages to OUT/tree (the repository layout), makes
//     ExpectProducer.fixture/ExpectProducer.jet the CLI package's entry and routes
//     the CLI's MIR request dispatch to it, and writes the producer's test fixture.
//     Then build the producer as the compiler is built:
//       JET_TOOLCHAIN_ROOT=<repo> jet build OUT/tree/Compiler
//     (the binary is OUT/tree/Compiler/JetCli/.jet/build/jet).
//   run-expect-producer.mjs OUT produce PRODUCER_BINARY | link | run
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import { compilerInputs, compilerPackages, WORKSPACE_FILES } from '../../../Tools/agent/compiler-packages.mjs';
import { spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, mode, producer] = process.argv.slice(2);
if (!output || !['assemble', 'produce', 'link', 'run'].includes(mode)) throw Error('usage: run-expect-producer.mjs OUT assemble|produce PRODUCER_BINARY|link|run');
const out = resolve(output), prefix = join(out, 'producer');
function command(name, binary, args, options = {}) {
  const run = spawnSync(binary, args, { cwd: root, encoding: 'utf8', maxBuffer: 1 << 27, ...options });
  writeFileSync(join(out, `${name}.receipt.json`), JSON.stringify({ status: run.status, signal: run.signal, error: run.error?.message ?? null, stdout: run.stdout, stderr: run.stderr }, null, 2) + '\n');
  if (run.error || run.signal || run.status !== 0) throw Error(`${name}: ${run.error?.message ?? run.status}\n${run.stdout ?? ''}${run.stderr ?? ''}`);
  return run;
}
if (mode === 'assemble') {
  const tree = join(out, 'tree');
  rmSync(tree, { recursive: true, force: true });
  const roots = [...WORKSPACE_FILES, ...[...compilerPackages(root).values()].map(directory => relative(root, directory))];
  for (const path of compilerInputs(root, roots)) {
    mkdirSync(dirname(join(tree, path)), { recursive: true });
    copyFileSync(join(root, path), join(tree, path));
  }
  const requestPath = join(tree, 'Compiler/JetCli/Source/Cli/Request.jet');
  const request = readFileSync(requestPath, 'utf8');
  const dispatch = 'else -> jet_driver_compile_mir(^request)';
  if (request.split(dispatch).length !== 2) throw Error('expected exactly one canonical CLI MIR request dispatch');
  writeFileSync(requestPath, request.replace(dispatch, 'else -> fx_expect_producer_compile(^request)'));
  copyFileSync(join(root, 'Compiler/JetBackend/Tests/ExpectProducer.fixture/ExpectProducer.jet'), join(tree, 'Compiler/JetCli/run.jet'));
  mkdirSync(join(out, 'fixture/Source'), { recursive: true });
  writeFileSync(join(out, 'fixture/package.jet'), 'name: "native_expect_producer"\nversion: "0.0.1"\nedition: "2026"\n');
  writeFileSync(join(out, 'fixture/Source/Test.jet'), 'struct Resource { name: String }\nimpl Resource.Close { fn close(^self) { print("close {self.name}") } }\n#Test("native expectation producer") {\n    .expect_fail(E3001, message: "producer boom") {\n        own := Resource{name: "own"}\n        defer close(^own)\n        panic("producer boom")\n    }\n    print("producer continued")\n}\n');
  console.log(`native expectation producer packages written; build: JET_TOOLCHAIN_ROOT=${root} jet build ${join(tree, 'Compiler')}`);
} else if (mode === 'produce') {
  if (!producer || !existsSync(resolve(producer))) throw Error('built producer PRODUCER_BINARY required');
  const run = command('produce', resolve(producer), [], { timeout: Number(process.env.JET_FRONT_TIMEOUT_MS ?? 600000), env: { ...process.env, JET_EXPECT_PRODUCER_SOURCE: join(out, 'fixture/Source/Test.jet'), JET_EXPECT_PRODUCER_OUTPUT: prefix } });
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
