#!/usr/bin/env node
// Exercise the actual canonical generator in temporary trees. No production
// output (especially the frozen ordinary projection) is written by this test.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const generator = 'Tools/agent/gen-runtime-trap-messages.mjs';
const frozen = 'Compiler/JetRuntime/Source/TrapMessages.jet';
const defects = 'Compiler/JetRuntime/Source/TrapDefectMessages.jet';
const inputs = [generator, frozen, 'Compiler/JetFoundation/Source/Registry/DiagnosticRows.jet', 'Compiler/JetFoundation/Source/Text/RustDebug.jet', 'Compiler/JetIR/Source/RouteCarriers.jet'];
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
function fixture(body) {
  const root = mkdtempSync(join(tmpdir(), 'jet-trap-defects-'));
  try {
    for (const path of inputs) {
      mkdirSync(dirname(join(root, path)), { recursive: true });
      copyFileSync(join(repo, path), join(root, path));
    }
    body(root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}
function run(root, ...args) {
  const result = spawnSync(process.execPath, [join(root, generator), ...args], { encoding: 'utf8' });
  assert.equal(result.error, undefined);
  assert.equal(result.signal, null);
  return result;
}
function success(root, ...args) {
  const result = run(root, '--defects-only', ...args);
  assert.equal(result.status, 0, result.stderr);
}
function stamp(root, path) {
  return { bytes: readFileSync(join(root, path)), mtime: statSync(join(root, path), { bigint: true }).mtimeNs };
}
function unchanged(root, path, old) {
  assert.deepEqual(readFileSync(join(root, path)), old.bytes);
  assert.equal(statSync(join(root, path), { bigint: true }).mtimeNs, old.mtime);
}
function row(source, name, sink) {
  const body = new RegExp(`^fn ${name}\\(\\) \\{\\n([\\s\\S]*?)^\\}`, 'm').exec(source)?.[1];
  assert.notEqual(body, undefined, name);
  const bytes = [];
  const chunks = [];
  for (const line of body.trim().split('\n')) {
    const word = new RegExp(`^${sink}\\(I64\\{(-?\\d+)\\}, I64\\{(\\d+)\\}\\)$`).exec(line.trim());
    assert.notEqual(word, null, line);
    const count = Number(word[2]);
    assert(count >= 1 && count <= 8);
    let value = BigInt.asUintN(64, BigInt(word[1]));
    for (let at = 0; at < count; at++, value >>= 8n) bytes.push(Number(value & 255n));
    chunks.push(count);
  }
  return { bytes: Buffer.from(bytes), chunks };
}

test('real source facts produce only canonical raw prefix/LF; frozen bytes and mtime are untouched', () => fixture(root => {
  const old = stamp(root, frozen);
  success(root);
  unchanged(root, frozen, old);
  const ordinary = old.bytes.toString('utf8');
  const generated = readFileSync(join(root, defects), 'utf8');
  const prefix = row(generated, 'trap_defect_unreachable_head', 'trap_write_word_raw');
  const newline = row(generated, 'trap_defect_newline', 'trap_write_word_raw');
  assert.deepEqual(prefix.bytes, row(ordinary, 'trap_msg_unreachable', 'trap_write_word').bytes);
  assert.deepEqual(newline.bytes, row(ordinary, 'trap_msg_newline', 'trap_write_word').bytes);
  assert.deepEqual(prefix.chunks, [8, 8, 8, 8, 8, 2]);
  assert.deepEqual(newline.chunks, [1]);
  assert.equal((generated.match(/^fn /gm) ?? []).length, 2);
  console.log(JSON.stringify({ inputs: inputs.map(path => ({ path, sha256: hash(readFileSync(join(root, path))) })), generatedSha256: hash(Buffer.from(generated)), prefixBytes: prefix.bytes.length, prefixChunks: prefix.chunks, newlineBytes: newline.bytes.length, generatedSource: generated }));
}));

test('unchanged generation and --check preserve both outputs byte-for-byte and mtime', () => fixture(root => {
  success(root);
  const ordinary = stamp(root, frozen), privateRows = stamp(root, defects);
  success(root);
  success(root, '--check');
  unchanged(root, frozen, ordinary);
  unchanged(root, defects, privateRows);
}));

test('any frozen mismatch fails before writing even a stale private output', () => fixture(root => {
  success(root);
  writeFileSync(join(root, frozen), Buffer.concat([readFileSync(join(root, frozen)), Buffer.from('\n')]));
  writeFileSync(join(root, defects), 'stale private projection\n');
  const ordinary = stamp(root, frozen), privateRows = stamp(root, defects);
  for (const args of [[], ['--check']]) {
    const result = run(root, '--defects-only', ...args);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /frozen output was not written/);
    unchanged(root, frozen, ordinary);
    unchanged(root, defects, privateRows);
  }
}));

test('--check never repairs stale private bytes; scoped generation repairs only private bytes', () => fixture(root => {
  success(root);
  const expected = readFileSync(join(root, defects));
  writeFileSync(join(root, defects), 'stale private projection\n');
  const ordinary = stamp(root, frozen), stale = stamp(root, defects);
  const result = run(root, '--defects-only', '--check');
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /TrapDefectMessages.jet is stale/);
  unchanged(root, frozen, ordinary);
  unchanged(root, defects, stale);
  success(root);
  unchanged(root, frozen, ordinary);
  assert.deepEqual(readFileSync(join(root, defects)), expected);
}));

test('ordinary default remains the canonical producer for both projections after the frozen gate', () => fixture(root => {
  const ordinary = readFileSync(join(root, frozen));
  success(root);
  const privateRows = readFileSync(join(root, defects));
  writeFileSync(join(root, frozen), 'stale ordinary projection\n');
  writeFileSync(join(root, defects), 'stale private projection\n');
  const result = run(root);
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(readFileSync(join(root, frozen)), ordinary);
  assert.deepEqual(readFileSync(join(root, defects)), privateRows);
  const checked = run(root, '--check');
  assert.equal(checked.status, 0, checked.stderr);
}));
