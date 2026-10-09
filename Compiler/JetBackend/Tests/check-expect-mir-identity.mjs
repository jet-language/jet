#!/usr/bin/env node
// Heavy composed proof: each checked-MIR driver must have been built from its
// respective pinned compiler source, not from the same current aggregate.
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { runNativeCheckedMir } from '../../../Tools/agent/compiler-native-transport.mjs';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, before, after] = process.argv.slice(2);
if (!output || !before || !after) throw Error('usage: check-expect-mir-identity.mjs OUT BEFORE_CHECKED_MIR_DRIVER AFTER_CHECKED_MIR_DRIVER');
const out = resolve(output);
for (const driver of [before, after]) if (!existsSync(resolve(driver))) throw Error(`missing pinned checked-MIR driver: ${driver}`);
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
  const file = join(unit, 'Source/Fixture.jet');
  writeFileSync(file, source);
  writeFileSync(join(unit, 'package.jet'), 'name: "expect_non_test_identity"\nversion: "0.0.1"\nedition: "2028"\n');
  const bytes = [];
  for (const [label, driver] of [['before', before], ['after', after]]) {
    const prefix = join(unit, label), dump = prefix + '.jet.mir';
    const result = runNativeCheckedMir({ driver: resolve(driver), file, mir: dump, prefix, cwd: unit, env: { ...process.env, JET_TOOLCHAIN_ROOT: root, JET_STORE_DIR: join(out, 'store', label) } });
    if (result.problem) throw Error(`${path} ${label}: ${result.problem.verdict}: ${result.problem.detail}`);
    const observed = readFileSync(dump);
    if (observed.length === 0) throw Error(`${path} ${label}: empty MIR observation`);
    bytes.push(observed);
  }
  const digests = bytes.map(value => createHash('sha256').update(value).digest('hex'));
  rows.push({ path, bytes: bytes[0].length, before: digests[0], after: digests[1], identical: bytes[0].equals(bytes[1]) });
}
const driver = path => ({ path: resolve(path), sha256: createHash('sha256').update(readFileSync(path)).digest('hex') });
writeFileSync(join(out, 'identity.json'), JSON.stringify({ mode: 'native-checked-mir', beforeDriver: driver(before), afterDriver: driver(after), corpus: rows }, null, 2) + '\n');
for (const row of rows) console.log(`${row.identical ? 'PASS' : 'FAIL'} ${row.path} ${row.before} ${row.after}`);
if (rows.some(row => !row.identical)) throw Error('non-test native MIR bytes changed across expectation producer amendment');
console.log('non-test native MIR byte identity: CHECK OK');
