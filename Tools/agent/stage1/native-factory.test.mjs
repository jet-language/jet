import test from 'node:test';
import assert from 'node:assert/strict';
import { admitFactoryReceipt } from './native-factory.mjs';
const receipt = 'mode=factory\ncomplete=true\nsource_bytes=0\nreport_count=0\nselected_factory_tier=Aot\nactual_factory_tier=Aot\n';
const emptyCensus = 'phase\titem\tcode\tmessage\tspan\n';
test('library/native receipt has no Rust-source byte requirement', () => {
  assert.equal(admitFactoryReceipt(receipt, emptyCensus).source_bytes, '0');
});
test('factory diagnostics never qualify as native/library success', () => {
  assert.throws(() => admitFactoryReceipt(receipt, emptyCensus + 'lower\tf\tE1234\tfailed\tx.jet:1\n'), /non-empty/);
  assert.throws(() => admitFactoryReceipt(receipt.replace('report_count=0', 'report_count=1'), emptyCensus), /report_count/);
  assert.throws(() => admitFactoryReceipt(receipt.replace('complete=true', 'complete=false'), emptyCensus), /complete/);
  assert.throws(() => admitFactoryReceipt(receipt, ''), /missing/);
});
test('an interpreter substitution is not the source-bound AOT factory', () => {
  assert.throws(() => admitFactoryReceipt(receipt.replace('actual_factory_tier=Aot', 'actual_factory_tier=Interpret'), emptyCensus), /actual_factory_tier/);
});
