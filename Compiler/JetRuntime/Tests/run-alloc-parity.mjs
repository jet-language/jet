#!/usr/bin/env node
// Jet-only allocator failure witness; the cross-tier reference is frozen.
// Standalone C links the supplied Jet-authored runtime, never a Rust provider.
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import { mkdirSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, ...inputs] = process.argv.slice(2);
if (inputs.includes('--rust-reference') || inputs.includes('--native')) {
  console.log('alloc-parity seed comparison: parked: Rust reference retired (owner #38/#59); Tower #4923');
  process.exit(0);
}
if (!output || !inputs.length) {
  throw new Error('usage: run-alloc-parity.mjs OUT JET_ONLY_RUNTIME_OBJECT_OR_ARCHIVE... [LINK_ARG...] | OUT --rust-reference');
}
const out = resolve(output);
mkdirSync(out, { recursive: true });
const fixture = join(root, 'Compiler/JetRuntime/Tests/CAbiDriver.c');
const binaries = {};
const errors = [];
const groups = [['native', inputs]];
for (const [tier, links] of groups) {
  const binary = join(out, `alloc-${tier}`);
  const args = ['-std=c11', '-Wall', '-Wextra', '-Werror', '-pedantic', '-DC_ABI_ALLOC_ONLY=1', fixture,
    ...links.map(input => input.startsWith('-') ? input : resolve(input)), '-lpthread', '-ldl', '-lm', '-o', binary];
  const built = spawnSync(nativeCCompiler, args);
  writeFileSync(join(out, `${tier}.build.stdout`), built.stdout ?? Buffer.alloc(0));
  writeFileSync(join(out, `${tier}.build.stderr`), built.stderr ?? Buffer.alloc(0));
  if (built.error || built.signal || built.status !== 0) {
    errors.push(`${tier} fixture build failed: ${built.error ?? built.signal ?? built.status}; see ${tier}.build.stderr`);
    continue;
  }
  binaries[tier] = binary;
}
const size = '9223372036854775800'; // isize::MAX - 7, aligned to 8 on x86-64.
const results = [];
for (const test of [
  { name: 'alloc-invalid-layout', status: 70, signal: null,
    prefix: Buffer.from('Stop [E3001]: `panic: invalid box layout`\n  --> <core.prelude>:0\n') },
  { name: 'alloc-failure', status: null, signal: 'SIGABRT',
    exact: Buffer.from(`memory allocation of ${size} bytes failed\n`) },
]) {
  const before = errors.length;
  for (const tier of ['native']) {
    if (!binaries[tier]) {
      errors.push(`${test.name}/${tier}: build failed`);
      continue;
    }
    const result = spawnSync(binaries[tier], [test.name]);
    writeFileSync(join(out, `${test.name}.${tier}.stdout`), result.stdout ?? Buffer.alloc(0));
    writeFileSync(join(out, `${test.name}.${tier}.stderr`), result.stderr ?? Buffer.alloc(0));
    writeFileSync(join(out, `${test.name}.${tier}.status.json`), JSON.stringify({ status: result.status, signal: result.signal, error: result.error?.message ?? null }) + '\n');
    if (result.error || result.status !== test.status || result.signal !== test.signal || (result.stdout?.length ?? 0) !== 0) {
      errors.push(`${test.name}/${tier}: unexpected status, signal, spawn error or stdout; see captured status/bytes`);
    }
    if (test.exact && !result.stderr?.equals(test.exact)) errors.push(`${test.name}/${tier}: allocation failure bytes differ from required diagnostic`);
    if (test.prefix && !result.stderr?.subarray(0, test.prefix.length).equals(test.prefix)) errors.push(`${test.name}/${tier}: invalid-layout diagnostic prefix differs`);
  }
  console.log('alloc-parity seed comparison: parked: Rust reference retired (owner #38/#59); Tower #4923');
  results.push(`${test.name}\t${errors.length === before ? 'PASS' : 'FAIL'}\t${test.status ?? ''}\t${test.signal ?? ''}`);
}
writeFileSync(join(out, 'alloc-parity.tsv'), 'case\tresult\tstatus\tsignal\n' + results.join('\n') + '\n');
console.log(results.join('\n'));
if (errors.length) throw new Error(errors.join('\n'));
