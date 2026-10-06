#!/usr/bin/env node
// Focused mechanical-schema tests. No compiler/runtime outputs are mocked.
import assert from 'node:assert/strict';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Converter, encode, loadMirSchema, parseRustDebug } from './mir-debug.mjs';
import { decodeStream, jetWriter } from './front-feed-codec.mjs';
const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const schema = loadMirSchema(repo);
const named = name => ({ k: 'named', name });
const renames = { MIRLocal: { is_comptime: 'comptime' }, MIRFunction: { module_name: 'module' }, MIRImport: { module_id: 'module' }, MIRTypeDef: { module_id: 'module' }, MIRPreludeCall: { module_name: 'module', effect_kind: 'effect' }, MIRCoreCall: { module_name: 'module', effect_kind: 'effect' } };
// Produce shape-complete inert nodes solely for exercising mechanical codec
// fields. Every semantic case under test replaces its relevant checked facts.
function node(type) {
  if (type.k === 'opt') return { k: 'unit', name: 'None' };
  if (type.k === 'list') return { k: 'list', items: [] };
  if (type.k === 'map') return { k: 'map', entries: [] };
  if (type.name === 'String') return { k: 'str', v: '' };
  if (type.name === 'Bool') return { k: 'bool', v: false };
  if (['Int', 'Float', 'U64', 'U32', 'U16', 'U8', 'I64', 'I32', 'I16', 'I8', 'Char'].includes(type.name)) return { k: 'num', v: '0' };
  const def = schema.get(type.name);
  assert.ok(def, `missing schema ${type.name}`);
  if (def.kind === 'struct') return { k: 'struct', name: def.name, fields: def.fields.filter(f => !(def.name === 'MIRFunction' && f.name === 'memo_unbounded')).map(f => [renames[def.name]?.[f.name] ?? f.name, node(f.type)]) };
  const variant = def.variants[0];
  return variant.fields.length ? { k: 'struct', name: variant.name, fields: variant.fields.map(f => [f.name, node(f.type)]) } : { k: 'unit', name: variant.name };
}
const set = (value, field, item) => { const row = value.fields.find(([key]) => key === field); assert.ok(row, `missing field ${field}`); row[1] = item; };
const field = (value, owner, name) => value.fields[schema.get(owner).fields.findIndex(f => f.name === name)];
const variant = (value, owner) => schema.get(owner).variants[value.tag].name;
const convert = (text, name, converter = new Converter(schema)) => converter.convert(parseRustDebug(text), named(name), 'test');

for (const [raw, bound, unbounded] of [['None', null, false], ['Some(None)', null, true], ['Some(Some(37))', '37', false]]) {
  const fn = node(named('MIRFunction')); set(fn, 'memo_bound', parseRustDebug(raw));
  const value = new Converter(schema).convert(fn, named('MIRFunction'), 'test');
  assert.equal(field(value, 'MIRFunction', 'memo_bound').opt?.int ?? null, bound);
  assert.equal(field(value, 'MIRFunction', 'memo_unbounded'), unbounded);
}
const local = node(named('MIRLocal')); set(local, 'comptime', { k: 'bool', v: true });
assert.equal(field(new Converter(schema).convert(local, named('MIRLocal'), 'test'), 'MIRLocal', 'is_comptime'), true);
const interpolation = convert('Value(MirValueId(23))', 'MIRStringPartKind');
assert.equal(variant(interpolation, 'MIRStringPartKind'), 'Interpolation');
assert.equal(variant(interpolation.fields[1], 'MIRStringFormat'), 'Display');
const imported = convert('Unqualified { module: MirModuleId(42), items: [] }', 'MIRImportKind');
assert.equal(imported.fields[0].fields[0].int, '42');
for (const fact of schema.get('MIRCopyFact').variants) {
  const copy = convert(`Copy { value: MirValueId(19), fact: ${fact.name} }`, 'MIROperation');
  assert.equal(variant(copy.fields[1], 'MIRCopyFact'), fact.name);
}
assert.throws(() => convert('Copy { value: MirValueId(19) }', 'MIROperation'), /missing checked Rust field/);
const binary = 'Binary { op: Add, dispatch: Prelude { call: MirPreludeCallId(17), location: MirPanicLoc { file: None, line: None }, fallibility: Infallible }, left: MirValueId(1), right: MirValueId(2) }';
assert.throws(() => convert(binary, 'MIROperation'), /whole program/);
const context = new Converter(schema);
const program = node(named('MIRProgram'));
const route = node(named('MIRPreludeCall'));
set(route, 'id', parseRustDebug('MirPreludeCallId(17)')); set(route, 'family', parseRustDebug('Overflow')); set(route, 'member', { k: 'str', v: 'u32.trap.add' });
set(program, 'prelude_calls', { k: 'list', items: [route] });
context.convert(program, named('MIRProgram'), 'test');
const primitive = convert('Binary { op: Add, dispatch: Primitive, left: MirValueId(1), right: MirValueId(2) }', 'MIROperation', context);
assert.equal(variant(primitive.fields[2], 'MIRBinaryOverflow'), 'Unchecked');
// The panic location/fallibility fields are taken from the real dispatch
// schema rather than hardcoded native spellings.
const checkedBinary = node(named('MIROperation'));
checkedBinary.name = 'Binary';
checkedBinary.fields = schema.get('MIROperation').variants.find(v => v.name === 'Binary').fields.filter(f => f.name !== 'overflow').map(f => [f.name, node(f.type)]);
const dispatch = schema.get('MIRBinaryDispatch').variants.find(v => v.name === 'Prelude');
set(checkedBinary, 'dispatch', { k: 'struct', name: 'Prelude', fields: dispatch.fields.map(f => [f.name, f.name === 'call' ? parseRustDebug('MirPreludeCallId(17)') : node(f.type)]) });
assert.equal(variant(context.convert(checkedBinary, named('MIROperation'), 'test').fields[2], 'MIRBinaryOverflow'), 'Trap');
set(route, 'member', { k: 'str', v: 'u32.wrapping.add' }); context.convert(program, named('MIRProgram'), 'test');
assert.equal(variant(context.convert(checkedBinary, named('MIROperation'), 'test').fields[2], 'MIRBinaryOverflow'), 'Unchecked');
const tokens = ['0']; encode(context.convert(program, named('MIRProgram'), 'test'), named('MIRProgram'), schema, tokens);
assert.equal(decodeStream(tokens.join('\n'), schema).program.schema_version, '0');
assert.throws(() => decodeStream(tokens.slice(0, -1).join('\n'), schema), /truncated/);
assert.throws(() => decodeStream(tokens.concat('trailing').join('\n'), schema), /trailing/);
const writer = jetWriter(schema);
assert.match(writer, /fn mirw_mir_program\(/);
assert.match(writer, /mirw_mir_copy_fact\(v1, &out\)/);
console.log('front-feed codec: all six checked schema mappings and strict observation passed');
