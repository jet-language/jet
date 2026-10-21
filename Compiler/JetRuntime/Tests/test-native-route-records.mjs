#!/usr/bin/env node
import assert from 'node:assert/strict';
import { nativeRecordExports, partitionNativeRecordExports } from '../../../Tools/agent/native-route-records.mjs';
import { classifyNativeInventory } from '../../../Tools/agent/classify-native-route-glue.mjs';

const types = Array.from({ length: 10 }, (_, index) => {
  const key = `<corelib>/core.fixture::Record${index}`;
  return { runtime: `Record${index}`, jetName: key, source: { key, path: 'Core/fixture/fixture.jet' },
    record: { kind: 'struct', members: [{ name: 'value', kind: ['Text', 'Int', 'Flag', 'Float', 'Tag.Color', 'Rec.Record0', 'OptText', 'OptInt', 'List.Byte', 'Entries.Rec.Record0'][index] }] } };
});
types[5].record = { kind: 'enum', members: [{ name: 'Absent', kind: null }, { name: 'Present', kind: 'Rec.Record0' }] };
types[0].fields = [{ name: 'label', kind: 'Text' }];
const descriptors = nativeRecordExports(types);
assert.equal(descriptors.length, 34);
assert.ok(descriptors.some((row) => row.name === 'jet_rt_record_new_record5_absent'));
assert.ok(!descriptors.some((row) => row.name === 'jet_rt_record_get_record5_absent'));
assert.ok(descriptors.some((row) => row.name === 'jet_rt_record_seq_entries_rec_record0_key'));
assert.ok(descriptors.some((row) => row.name === 'jet_rt_handle_field_record0_label'));
const repeated = nativeRecordExports([...types, { ...types[8], runtime: 'Another', record: types[8].record }]);
assert.equal(repeated.filter((row) => row.name === 'jet_rt_record_seq_list_byte_new').length, 1);
assert.deepEqual(nativeRecordExports([{ runtime: 'RustOnly', jetName: 'RustOnly' }]), []);
const callers = new Map([
  ['jet_rt_record_get_record0_value', ['native.external.read', 'native.external.read']],
  ['jet_rt_record_new_record5_present', ['native.leaf.create']],
]);
const partition = partitionNativeRecordExports(types, callers);
assert.equal(partition.synthesized.length, 2);
assert.equal(partition.deleted.length, 32);
assert.deepEqual(partition.synthesized.find((row) => row.op === 'record.get').callers, ['native.external.read']);
assert.throws(() => partitionNativeRecordExports(types, new Map([['jet_rt_record_new_record0', ['']]])), /caller witness/);
assert.throws(() => nativeRecordExports([types[0], types[0]]), /duplicate ownership/);
assert.throws(() => nativeRecordExports([{ ...types[0], record: { kind: 'struct', members: [{ name: 'bad', kind: 'Entries.Byte' }] } }]), /unsupported carrier/);
assert.throws(() => nativeRecordExports([{ ...types[0], record: { kind: 'struct', members: [{ name: 'bad', kind: null }] } }]), /no carrier/);

const source = types.map((type, index) => `pub struct Record${index} { value: Int }`).join('\n');
const routes = types.map((type) => `LowerRuntimeHandle{jet_name: "${type.jetName}", runtime: "${type.runtime}"},\nLowerRecordSchema{jet_name: "${type.runtime}", variants: "", fields: "value=Int"},`).join('\n') + '\nLowerRuntimeHandle{jet_name: "RustOnly", runtime: "RustOnly"},';
const inventory = 'symbol\tlayer\n' + descriptors.map((row) => `${row.name}\tcabi-glue`).join('\n') + '\njet_rt_handle_drop_rust_only\tcabi-glue\n';
const report = classifyNativeInventory(inventory, routes, (path) => path === 'Core/fixture/fixture.jet' ? source : null);
assert.equal(report.counts.types, 11);
assert.equal(report.counts.sourceRecordCandidates, 10);
assert.equal(report.counts.rustOnlyTypes, 1);
assert.equal(report.counts.symbols, 24); // Sequence holders do not claim a declaration owner.
assert.equal(report.counts.unclassifiedSymbols, 11);
assert.ok(report.types.every((row) => row.nativeLayout.startsWith('unexercised:')));
console.log('CHECK OK — native record metadata: 10 source types, 34 adapters, 32 deleted, 2 synthesized caller witnesses; conditional classifier, no layout claim');
