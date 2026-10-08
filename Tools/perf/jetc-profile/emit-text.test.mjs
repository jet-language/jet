import { test } from 'node:test';
import assert from 'node:assert/strict';
import { helpers, compare } from './emit-text.mjs';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const output = (fold, unwrap, compact, body = 'same text') => `emit-text regressions ok\n${body}\n@@time fold ${fold}\n@@time unwrap ${unwrap}\n@@time compact ${compact}\n`;

test('emit pair requires complete matched positive trials and exact output', () => {
  assert.throws(() => compare([output(9, 9, 9)], [output(8, 8, 8)]), /three matched/);
  assert.throws(() => compare(Array(3).fill(output(9, 9, 9)), Array(3).fill(output(8, 8, 8, 'changed'))), /differs/);
  assert.throws(() => compare(Array(3).fill(output(9, 9, 9)), Array(3).fill(output(0, 8, 8))), /non-positive/);
  assert.throws(() => compare(Array(3).fill(output(9, 9, 9)), Array(3).fill('@@time fold 8\n')), /missing/);
});

test('each pass must strictly win; equality and a single loss stay open', () => {
  const base = Array(3).fill(output(9, 9, 9));
  assert.equal(compare(base, Array(3).fill(output(8, 8, 8))).verdict, 'win');
  const equal = compare(base, Array(3).fill(output(8, 9, 8)));
  assert.equal(equal.verdict, 'loss');
  assert.equal(equal.metrics.unwrap.verdict, 'loss');
  assert.equal(compare(base, Array(3).fill(output(8, 10, 8))).verdict, 'loss');
});

test('helper extraction excludes MIR-dependent localization without parsing text braces', () => {
  const source = '\nfn jet_rust_emit_blank_line() { "{{" }\nstruct JetRustEmitLocalizedSlots { ctx: MIR }\nfn jet_rust_emit_declared_value_id() { "}}" }\nfn jet_rust_emit_function_signature() { MIR }';
  const extracted = helpers(source);
  assert.ok(extracted.includes('jet_rust_emit_blank_line'));
  assert.ok(extracted.includes('jet_rust_emit_declared_value_id'));
  assert.ok(!extracted.includes('MIR'));
  assert.throws(() => helpers('missing'), /boundaries/);
});

test('filesystem compare CLI matches .out trials and publishes valid immutable JSON', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'jet-emit-text-test-'));
  try {
    const program = 'paired program bytes';
    const program_sha256 = crypto.createHash('sha256').update(program).digest('hex');
    const pair = { schema: 'jet.emit-text-pair.v1', arms: {} };
    for (const arm of ['plain', 'optimized']) {
      const dir = path.join(root, arm);
      fs.mkdirSync(dir);
      fs.writeFileSync(path.join(dir, 'main.jet'), program);
      pair.arms[arm] = { program_sha256 };
      for (let trial = 1; trial <= 3; trial++) {
        fs.writeFileSync(path.join(dir, `trial-00${trial}.out`), arm === 'plain' ? output(9, 9, 9) : output(8, 8, 8));
      }
      fs.writeFileSync(path.join(dir, 'trial-999\\Xout'), 'not a trial');
    }
    fs.writeFileSync(path.join(root, 'pair.json'), JSON.stringify(pair));
    const receipt = path.join(root, 'receipt.json');
    const cli = fileURLToPath(new URL('./emit-text.mjs', import.meta.url));
    const invoke = target => spawnSync(process.execPath, [cli, 'compare', root, target], { encoding: 'utf8' });
    const result = invoke(receipt);
    assert.equal(result.status, 0, result.stderr);
    const bytes = fs.readFileSync(receipt, 'utf8');
    assert.ok(bytes.endsWith('\n'));
    const parsed = JSON.parse(bytes);
    assert.equal(parsed.verdict, 'win');
    assert.deepEqual(parsed.trial_files, ['trial-001.out', 'trial-002.out', 'trial-003.out']);
    assert.notEqual(invoke(receipt).status, 0, 'an existing receipt must not be overwritten');
    fs.unlinkSync(path.join(root, 'optimized', 'trial-003.out'));
    const unmatched = invoke(path.join(root, 'unmatched.json'));
    assert.notEqual(unmatched.status, 0);
    assert.match(unmatched.stderr, /unmatched trial files/);
    fs.writeFileSync(path.join(root, 'optimized', 'trial-003.out'), output(8, 8, 8));
    fs.writeFileSync(path.join(root, 'optimized', 'main.jet'), 'changed');
    const changed = invoke(path.join(root, 'changed.json'));
    assert.notEqual(changed.status, 0);
    assert.match(changed.stderr, /program changed/);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});
