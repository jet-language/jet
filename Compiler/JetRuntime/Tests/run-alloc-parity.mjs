#!/usr/bin/env node
// Cross-tier allocator failure witness; no Cargo harness or runtime rebuild.
// The supplied pack rlib must be built with --cfg jet_native_pack.
import { mkdirSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, rustRlib, packRlib, jetObject, ...dependencyDirs] = process.argv.slice(2);
if (!output || !rustRlib || !packRlib || !jetObject) {
  throw new Error('usage: run-alloc-parity.mjs OUT RUST_TIER_RLIB PACK_RLIB JET_OBJECT [DEPENDENCY_DIR...]');
}
const out = resolve(output);
mkdirSync(out, { recursive: true });
const fixture = join(root, 'tests/fixtures/runtime_c_abi_driver.rs');
const binaries = {};
const errors = [];
for (const [tier, rlib] of [['rust', rustRlib], ['pack', packRlib]]) {
  const binary = join(out, `alloc-${tier}`);
  const args = ['--edition=2021', '--cfg', 'runtime_alloc_failure_only', fixture,
    '--extern', `jet_runtime=${resolve(rlib)}`, '-o', binary];
  for (const directory of new Set([dirname(resolve(rlib)), ...dependencyDirs.map((path) => resolve(path))])) {
    args.push('-L', `dependency=${directory}`);
  }
  if (tier === 'pack') args.push('--cfg', 'jet_native_pack', '-C', `link-arg=${resolve(jetObject)}`);
  const built = spawnSync(process.env.RUSTC ?? 'rustc', args);
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
  const captures = {};
  const before = errors.length;
  for (const tier of ['rust', 'pack']) {
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
    captures[tier] = result;
  }
  if (captures.pack && captures.rust && !captures.pack.stderr?.equals(captures.rust.stderr)) errors.push(`${test.name}: Rust/Jet stderr bytes differ`);
  results.push(`${test.name}\t${errors.length === before ? 'PASS' : 'FAIL'}\t${test.status ?? ''}\t${test.signal ?? ''}`);
}
writeFileSync(join(out, 'alloc-parity.tsv'), 'case\tresult\tstatus\tsignal\n' + results.join('\n') + '\n');
console.log(results.join('\n'));
if (errors.length) throw new Error(errors.join('\n'));
