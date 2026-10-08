import { test } from 'node:test';
import assert from 'node:assert/strict';
import { helpers, compare } from './emit-text.mjs';

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
