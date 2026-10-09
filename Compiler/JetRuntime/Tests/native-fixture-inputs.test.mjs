import assert from 'node:assert/strict';
import {copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync} from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import test from 'node:test';

const here = path.dirname(fileURLToPath(import.meta.url));
function scratch(body) {
  const directory = mkdtempSync(path.join(os.tmpdir(), 'jet-native-fixture-inputs-'));
  try { body(directory); } finally { rmSync(directory, {recursive: true, force: true}); }
}
function scan(directory, optional = []) {
  const native = path.join(directory, 'native.tsv');
  writeFileSync(native, readFileSync(path.join(here, 'scan.cases'), 'utf8').trim().split('\n').map(stem => `${stem}\tpass`).join('\n') + '\n');
  const env = {...process.env};
  delete env.JET_RUNTIME_JET_OBJECT;
  delete env.JET_RUNTIME_PACK;
  return spawnSync(process.execPath, [path.join(here, 'run-scan-goldens.mjs'), path.join(directory, 'out'), native, ...optional], {env, encoding: 'utf8'});
}

test('native scan CLI does not require retired tier receipts', () => scratch(directory => {
  const result = scan(directory);
  assert.equal(result.status, 1);
  // This is an input-grammar fixture, not a native execution claim: it must
  // advance to the truthful missing-pack guard without requiring Rust inputs.
  assert.match(result.stderr, /fresh JET_RUNTIME_JET_OBJECT and JET_RUNTIME_PACK required/);
  assert.doesNotMatch(result.stderr, /usage:/);
}));

test('saved scan comparisons use explicit frozen input names', () => scratch(directory => {
  const receipt = path.join(directory, 'frozen.tsv');
  writeFileSync(receipt, readFileSync(path.join(here, 'scan.cases'), 'utf8').trim().split('\n').map(stem => `${stem}\tpass`).join('\n') + '\n');
  const result = scan(directory, ['--frozen-reference', receipt, '--frozen-evaluator', receipt]);
  assert.match(result.stderr, /fresh JET_RUNTIME_JET_OBJECT and JET_RUNTIME_PACK required/);
  assert.doesNotMatch(result.stderr, /usage:/);
}));

test('scan rejects legacy positional and duplicate or incomplete frozen inputs', () => scratch(directory => {
  for (const optional of [['reference.tsv', 'evaluator.tsv'], ['--frozen-reference'], ['--frozen-reference', 'a', '--frozen-reference', 'b'], ['--live-reference', 'a']]) {
    const result = scan(directory, optional);
    assert.equal(result.status, 1);
    assert.match(result.stderr, /usage: run-scan-goldens/);
  }
}));

test('official BLAKE3 generation has no retired compiler/runtime source dependency', () => scratch(directory => {
  const tests = path.join(directory, 'Compiler', 'JetRuntime', 'Tests');
  mkdirSync(tests, {recursive: true});
  for (const file of ['generate-blake3-fixtures.mjs', 'Blake3OfficialVectors.json']) copyFileSync(path.join(here, file), path.join(tests, file));
  const result = spawnSync(process.execPath, [path.join(tests, 'generate-blake3-fixtures.mjs')], {encoding: 'utf8'});
  assert.equal(result.status, 0, result.stderr);
  for (const file of ['CryptoBlake3Goldens.h', 'CryptoBlake3Witness.stdout']) assert.deepEqual(readFileSync(path.join(tests, file)), readFileSync(path.join(here, file)), file);
  const generated = JSON.parse(readFileSync(path.join(tests, 'CryptoBlake3Fixtures.json'), 'utf8'));
  const frozen = JSON.parse(readFileSync(path.join(here, 'CryptoBlake3Fixtures.json'), 'utf8'));
  assert.deepEqual(generated.officialKAT, frozen.officialKAT);
  assert.equal(generated.stdoutSha256, frozen.stdoutSha256);
  assert.equal(Object.hasOwn(generated, 'seed'), false);
}));
