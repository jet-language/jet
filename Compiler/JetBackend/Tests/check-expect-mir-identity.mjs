#!/usr/bin/env node
// Heavy composed-proof item, not a light parser check. Both shared candidates
// must carry the f58ecd4f0 MIR observer; BEFORE is fbb0965fd plus observer only,
// AFTER carries the producer amendment. The same source roots are used twice.
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { frontReceipt } from './jet-front.mjs';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, before, after] = process.argv.slice(2);
if (!output || !before || !after) throw Error('usage: check-expect-mir-identity.mjs OUT BEFORE_COMPILER AFTER_COMPILER');
const out = resolve(output), pack = process.env.JET_RUNTIME_PACK;
if (!pack || !existsSync(pack)) throw Error('fresh composed JET_RUNTIME_PACK required');
for (const compiler of [before, after]) if (!existsSync(resolve(compiler))) throw Error(`missing shared compiler: ${compiler}`);
mkdirSync(out, { recursive: true });
const corpus = [
  'Examples/features/memory/release_defer_lifo.jet',
  'Examples/features/memory/release_defer_panic.jet',
  'Examples/features/memory/rawptr.jet',
];
const rows = [];
for (const [index, path] of corpus.entries()) {
  const unit = join(out, `case-${index}`), source = readFileSync(join(root, path), 'utf8');
  if (source.includes('#Test') || source.includes('.expect_fail')) throw Error(`identity corpus must be non-test: ${path}`);
  mkdirSync(join(unit, 'Source'), { recursive: true });
  writeFileSync(join(unit, 'Source/Fixture.jet'), source);
  writeFileSync(join(unit, 'package.jet'), 'name: "expect_non_test_identity"\nversion: "0.0.1"\nedition: "2028"\n');
  const bytes = [];
  for (const [label, compiler] of [['before', before], ['after', after]]) {
    const prefix = join(unit, label), receipt = prefix + '.receipt', dump = prefix + '.jet.mir';
    const env = { ...process.env };
    for (const key of ['JET_FRONT_LIBRARY_OBJECT_OUTPUT', 'JET_FRONT_LIBRARY_EXPORTS', 'JET_EXPECT_PRODUCER_OUTPUT', 'JET_EXPECT_PRODUCER_SOURCE']) delete env[key];
    const run = spawnSync(resolve(compiler), [], { cwd: root, env: { ...env, JET_BOOTSTRAP_MODE: 'factory', JET_BOOTSTRAP_FACTORY_TIER: 'aot',
      JET_BOOTSTRAP_SOURCE_ROOT: unit, JET_BOOTSTRAP_ENTRY: 'Source/Fixture.jet', JET_BOOTSTRAP_OUTPUT: prefix + '.unused',
      JET_BOOTSTRAP_RECEIPT: receipt, JET_FRONT_NATIVE_OUTPUT: prefix, JET_FRONT_MIR_DUMP: dump,
      JET_RUNTIME_PACK: pack, JET_STORE_DIR: join(out, 'store', label), RUST_MIN_STACK: '8388608' },
      timeout: Number(process.env.JET_FRONT_TIMEOUT_MS ?? 600000), encoding: 'utf8', maxBuffer: 1 << 27 });
    writeFileSync(prefix + '.run.json', JSON.stringify({ status: run.status, signal: run.signal, error: run.error?.message ?? null, stdout: run.stdout, stderr: run.stderr }, null, 2) + '\n');
    const problem = frontReceipt(existsSync(receipt) ? readFileSync(receipt, 'utf8') : '', run);
    if (problem) throw Error(`${path} ${label}: ${problem.verdict}: ${problem.detail}`);
    for (const suffix of ['.issues', '.pack-issues']) if (existsSync(prefix + suffix)) throw Error(readFileSync(prefix + suffix, 'utf8'));
    if (!existsSync(dump)) throw Error(`${path} ${label}: no canonical JET_FRONT_MIR_DUMP (observer must be backported to baseline)`);
    const observed = readFileSync(dump);
    if (observed.length === 0) throw Error(`${path} ${label}: empty MIR observation`);
    bytes.push(observed);
  }
  const digests = bytes.map(value => createHash('sha256').update(value).digest('hex'));
  rows.push({ path, bytes: bytes[0].length, before: digests[0], after: digests[1], identical: bytes[0].equals(bytes[1]) });
}
writeFileSync(join(out, 'identity.json'), JSON.stringify({ mode: 'native-stop-points', beforeCompiler: resolve(before), afterCompiler: resolve(after), corpus: rows }, null, 2) + '\n');
for (const row of rows) console.log(`${row.identical ? 'PASS' : 'FAIL'} ${row.path} ${row.before} ${row.after}`);
if (rows.some(row => !row.identical)) throw Error('non-test native MIR bytes changed across expectation producer amendment');
console.log('non-test native MIR byte identity: CHECK OK');
