#!/usr/bin/env node
// This historical witness requires a Rust harness; no native C mode exists here.
console.log("parked: Rust reference retired (owner #38/#59); Tower #4923");
process.exit(0);
// A narrow seed-oracle/native witness. The producer must supply this exact
// tree's freshly emitted Jet object and same-tree post-E2 runtime pack.
import { existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output] = process.argv.slice(2);
if (!output || process.argv.length !== 3) throw new Error('usage: run-utf8-corpus.mjs OUT');
const object = process.env.JET_RUNTIME_JET_OBJECT;
const pack = process.env.JET_RUNTIME_PACK;
if (!object || !pack || !existsSync(object) || !existsSync(pack)) {
  throw new Error('JET_RUNTIME_JET_OBJECT and JET_RUNTIME_PACK must name fresh same-tree Jet artifacts');
}
const out = resolve(output);
mkdirSync(out, { recursive: true });
const failures = [];
for (const [label, path] of [['object', object], ['pack', pack]]) {
  const symbols = spawnSync('nm', ['-g', '--defined-only', '--format=posix', path], { encoding: 'utf8', maxBuffer: 1 << 28 });
  writeFileSync(join(out, `${label}.symbols`), `${symbols.stdout ?? ''}${symbols.stderr ?? ''}`);
  if (symbols.error || symbols.status !== 0) failures.push(`${label} nm: ${symbols.error?.message ?? symbols.stderr}`);
  const definitions = (symbols.stdout ?? '').split('\n').map(line => line.trim().split(/\s+/u));
  const count = definitions.filter(([name, kind]) => name === 'jet_string_validate_utf8' && kind === 'T').length;
  if (count !== 1) failures.push(`${label}: validator has ${count} strong definitions`);
}
const executable = join(out, 'utf8-corpus');
let compile;
let run;
if (!failures.length) {
  compile = spawnSync(process.env.RUSTC ?? 'rustc', [
    '--edition=2024', join(root, 'Compiler/JetRuntime/Tests/utf8-corpus.rs'),
    '-C', 'panic=abort', '-C', `link-arg=${resolve(pack)}`,
    '-C', 'link-arg=-lpthread', '-C', 'link-arg=-ldl', '-C', 'link-arg=-lm',
    '-o', executable,
  ], { encoding: 'utf8', maxBuffer: 1 << 28 });
  writeFileSync(join(out, 'compile.log'), `${compile.stdout ?? ''}${compile.stderr ?? ''}`);
  if (compile.error || compile.status !== 0) failures.push(`compile: ${compile.error?.message ?? `rc=${compile.status}`}`);
  else {
    run = spawnSync(executable, [], { encoding: 'utf8', timeout: 60000 });
    writeFileSync(join(out, 'run.stdout'), run.stdout ?? '');
    writeFileSync(join(out, 'run.stderr'), run.stderr ?? '');
    if (run.error || run.status !== 0 || run.signal || run.stderr ||
        run.stdout !== 'UTF-8 Jet/native acceptance and exact errors: passed\n') {
      failures.push(`run: ${run.error?.message ?? `rc=${run.status}, signal=${run.signal}`}`);
    }
  }
}
const receipt = { object, pack, compileStatus: compile?.status ?? null, runStatus: run?.status ?? null, failures };
writeFileSync(join(out, 'receipt.json'), `${JSON.stringify(receipt, null, 2)}\n`);
console.log(JSON.stringify(receipt));
if (failures.length) process.exitCode = 1;
