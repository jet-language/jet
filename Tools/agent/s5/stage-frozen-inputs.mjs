#!/usr/bin/env node
// Copy only preserved data captures. Proof jobs never read their scratch origins.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {assertPinnedSource} from '../proof-source.mjs';
const [head, source, output] = process.argv.slice(2);
if (!/^[a-f0-9]{40}$/.test(head ?? '') || !source || !output || process.argv.length !== 5) throw Error('usage: stage-frozen-inputs.mjs FULL_SHA SOURCE_DIRECTORY IMMUTABLE_INPUT_DIRECTORY');
assertPinnedSource(source, head);
const table = JSON.parse(fs.readFileSync(path.join(source, 'Compiler/JetRuntime/Tests/AgeV1Fixtures.json'), 'utf8'));
const files = [
  {path: 'char-debug-goldens.bin', origin: '/home/nate/.cache/jet-dev/scratch/RtPlot1b/char-debug-goldens.bin', sha256: '72a498085e0dc43fc6f9b0bc6f29482738c01ab7a35c11d72a1cfaf2504807a5'},
  {path: 'fmt-expected.out', origin: '/home/nate/.cache/jet-dev/scratch/RtL6b3/fmt-parity/expected.out', sha256: 'f7c9a21168359f638c75fc031072856f283b5b5da6f9b93d55d618752cf1b54c'},
];
for (const row of table.cases.filter(row => row.name.startsWith('rust-0.10.1-'))) {
  const length = row.name.slice('rust-0.10.1-'.length);
  for (const [extension, sha256] of [['age', row.cipherSha256], ['plain', row.plainSha256]]) {
    const file = `age-rust-${length}.${extension}`;
    files.push({path: `age-rust/${file}`, origin: `/home/nate/.cache/jet-dev/scratch/RtCrypto1/frozen/${file}`, sha256});
  }
}
if (files.length !== 14 || files.some(file => !/^[a-f0-9]{64}$/.test(file.sha256))) throw Error('preserved input census or digest drift');
const hash = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const destination = path.resolve(output);
const manifest = {head, files, missing: [{runner: 'run-display-goldens.mjs', inputs: ['rust.stdout', 'rust32.stdout', 'rust-benchmark.stdout'], because: 'Rust-captured oracle not preserved'}, {runner: 'freeze-age-fixtures.mjs', inputs: ['age testkit/'], because: 'Rust-captured oracle not preserved'}]};
if (fs.existsSync(destination)) {
  const actual = JSON.parse(fs.readFileSync(path.join(destination, 'manifest.json'), 'utf8'));
  if (JSON.stringify(actual) !== JSON.stringify(manifest)) throw Error('immutable input manifest changed');
  for (const file of files) if (hash(path.join(destination, file.path)) !== file.sha256) throw Error(`immutable input changed: ${file.path}`);
  console.log(destination); process.exit(0);
}
const staging = `${destination}.staging-${process.pid}`;
fs.mkdirSync(staging, {recursive: true});
for (const file of files) {
  if (hash(file.origin) !== file.sha256) throw Error(`preserved capture changed: ${file.origin}`);
  const target = path.join(staging, file.path);
  fs.mkdirSync(path.dirname(target), {recursive: true}); fs.copyFileSync(file.origin, target);
  if (hash(target) !== file.sha256) throw Error(`input copy differs: ${file.path}`);
  fs.chmodSync(target, 0o444);
}
fs.writeFileSync(path.join(staging, 'manifest.json'), JSON.stringify(manifest, null, 2) + '\n', {mode: 0o444});
fs.chmodSync(path.join(staging, 'age-rust'), 0o555); fs.chmodSync(staging, 0o555);
fs.renameSync(staging, destination);
console.log(destination);
