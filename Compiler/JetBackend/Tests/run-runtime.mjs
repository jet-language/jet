#!/usr/bin/env node
// The hosted CLI checks the canonical runtime source aggregate, then its
// library hook lowers the checked exports and lifecycle as ONE native object.
// No Tests.rs factory, emitted Rust, or Rust schema reader enters this path.
// usage: JET=<hosted compiler> node run-runtime.mjs OUT
//        [--stage assemble|lower] [--until assemble|lower] [--test NAME]
import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const args = process.argv.slice(2);
const output = args.shift();
if (!output) throw Error('usage: run-runtime.mjs OUT [--stage assemble|lower] [--until assemble|lower] [--test NAME]');
const out = resolve(output);
const stages = ['assemble', 'lower'];
let first = 'assemble', last = 'lower', test = null;
for (let index = 0; index < args.length; index++) {
  if (args[index] === '--stage') first = args[++index];
  else if (args[index] === '--until') last = args[++index];
  else if (args[index] === '--test' && test === null && /^[A-Z][A-Za-z0-9]*$/u.test(args[index + 1] ?? '')) test = args[++index];
  else throw Error(`unknown argument: ${args[index]}`);
}
const from = stages.indexOf(first), until = stages.indexOf(last);
if (from < 0 || until < from) throw Error(`bad stage range ${first}..${last}`);
const library = join(out, 'library');
mkdirSync(library, { recursive: true });
const entry = join(library, 'Source/Runtime.jet');
if (from === 0) {
  execFileSync(process.execPath, [join(repo, 'Compiler/JetRuntime/Tests/check-runtime.mjs'), library, '--assemble-only', ...(test ? ['--test', test] : [])], { stdio: 'inherit' });
  mkdirSync(dirname(entry), { recursive: true });
  renameSync(join(library, 'unit.jet'), entry);
  writeFileSync(join(out, 'runtime.lifecycle'), readFileSync(join(repo, 'Compiler/JetRuntime/lifecycle.list')));
}
if (until === 1) {
  if (!process.env.JET) throw Error('JET must name the actual Jet-hosted CLI from this source tree');
  const prefix = join(out, 'jet_runtime_jet');
  for (const suffix of ['.o', '.exports', '.abi.json', '.issues', '.pack-issues']) rmSync(prefix + suffix, { force: true });
  const run = spawnSync(resolve(process.env.JET), ['check', entry], {
    cwd: library,
    env: { ...process.env, JET_TOOLCHAIN_ROOT: repo,
      JET_FRONT_LIBRARY_OBJECT_OUTPUT: prefix,
      JET_FRONT_LIBRARY_LIFECYCLE: join(repo, 'Compiler/JetRuntime/lifecycle.list'),
      JET_STORE_DIR: resolve(process.env.JET_STORE_DIR ?? join(out, 'store')),
    },
    encoding: 'utf8', timeout: Number(process.env.JET_FRONT_TIMEOUT_MS ?? 120000), maxBuffer: 1 << 27,
  });
  writeFileSync(join(out, 'front.log'), `${run.stdout ?? ''}${run.stderr ?? ''}`);
  if (run.error) throw run.error;
  if (run.status !== 0) throw Error(`hosted runtime lowering failed (${run.status}): ${run.stderr ?? ''}`);
  if (existsSync(prefix + '.issues')) throw Error(readFileSync(prefix + '.issues', 'utf8'));
  for (const suffix of ['.o', '.exports', '.abi.json']) {
    if (!existsSync(prefix + suffix)) throw Error(`hosted compiler did not produce ${prefix + suffix}; build the CLI candidate from this tree`);
  }
}
console.log(until === 1 ? join(out, 'jet_runtime_jet.o') : entry);
