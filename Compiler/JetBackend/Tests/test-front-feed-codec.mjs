#!/usr/bin/env node
// Focused mechanical-schema tests. No compiler/runtime outputs are mocked.
import assert from 'node:assert/strict';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Converter, encode, jetDecoder, loadMirSchema, parseJetSchema, parseRustDebug } from './mir-debug.mjs';
import { decodeStream, jetWriter } from './front-feed-codec.mjs';
import { nativeCommandProblem } from '../../../Tools/agent/compiler-native-transport.mjs';
const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const schema = loadMirSchema(repo);
// Native command status is strict: errors, signals, and nonzero exits never count as passes.
assert.equal(nativeCommandProblem({ status: 0 }), null);
assert.equal(nativeCommandProblem({ status: 1 }).verdict, 'no-mir');
assert.equal(nativeCommandProblem({ status: null, signal: 'SIGKILL' }).verdict, 'no-mir');
const named = name => ({ k: 'named', name });
// MIR NameLedger rows have declaration defaults. The codec must consume their
// types, never manufacture decoder names from the default initializer text.
const defaults = parseJetSchema(`struct DefaultFields {
    namespace: String?{None}
    paths: [String]{[String]{}}
    lookup: [String:[String?]]{[String:[String?]]{}}
}`);
assert.deepEqual(defaults.get('DefaultFields').fields.map(f => f.type), [
  { k: 'opt', of: named('String') },
  { k: 'list', of: named('String') },
  { k: 'map', key: named('String'), value: { k: 'list', of: { k: 'opt', of: named('String') } } },
]);
const decodedDefaults = jetDecoder(defaults);
assert.match(decodedDefaults, /mird_opt_string\(&r\)/);
assert.match(decodedDefaults, /mird_list_string\(&r\)/);
assert.doesNotMatch(decodedDefaults, /mird_string\?\{|mird_\[|none\}/i);
// The Rust half never carries Jet-only defaulted fields: an empty default fills
// them; a non-empty initializer is refused, never guessed.
assert.deepEqual(new Converter(defaults).convert(parseRustDebug('DefaultFields { }'), named('DefaultFields'), 'test'), { fields: [{ opt: null }, [], { map: [] }] });
const valued = parseJetSchema(`struct ValuedDefault {
    limit: Int{3}
}`);
assert.throws(() => new Converter(valued).convert(parseRustDebug('ValuedDefault { }'), named('ValuedDefault'), 'test'), /missing checked Rust field/);
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

// This checked display row is retained from modules/generic_modules' MIR dump.
// Its module field follows the same bootstrap mapping as declarations/aliases.
const displayPath = convert(`NameDisplayPath {
    module: 0,
    internal_path: "generic_modules.M3Int4LawsReveal",
    display_path: "int_laws.Reveal",
}`, 'MIRNameDisplayPath');
assert.equal(field(displayPath, 'MIRNameDisplayPath', 'module_index').int, '0');
assert.equal(field(displayPath, 'MIRNameDisplayPath', 'internal_path'), 'generic_modules.M3Int4LawsReveal');
assert.equal(field(displayPath, 'MIRNameDisplayPath', 'display_path'), 'int_laws.Reveal');
const displayTokens = [];
encode(displayPath, named('MIRNameDisplayPath'), schema, displayTokens);
assert.deepEqual(displayTokens, ['0', 'sgeneric_modules.M3Int4LawsReveal', 'sint_laws.Reveal']);

for (const [raw, bound, unbounded] of [['None', null, false], ['Some(None)', null, true], ['Some(Some(37))', '37', false]]) {
  const fn = node(named('MIRFunction')); set(fn, 'memo_bound', parseRustDebug(raw));
  const value = new Converter(schema).convert(fn, named('MIRFunction'), 'test');
  assert.equal(field(value, 'MIRFunction', 'memo_bound').opt?.int ?? null, bound);
  assert.equal(field(value, 'MIRFunction', 'memo_unbounded'), unbounded);
}
const local = node(named('MIRLocal')); set(local, 'comptime', { k: 'bool', v: true });
assert.equal(field(new Converter(schema).convert(local, named('MIRLocal'), 'test'), 'MIRLocal', 'is_comptime'), true);
for (const [raw, expected] of [['None', null], ['Some(MirValueId(23))', '23']]) {
  const bound = node(named('MIRLocal')); set(bound, 'ssa_value', parseRustDebug(raw));
  const value = new Converter(schema).convert(bound, named('MIRLocal'), 'test');
  assert.equal(field(value, 'MIRLocal', 'ssa_value').opt?.fields[0].int ?? null, expected);
}
const missingSsa = node(named('MIRLocal'));
missingSsa.fields = missingSsa.fields.filter(([name]) => name !== 'ssa_value');
assert.throws(() => new Converter(schema).convert(missingSsa, named('MIRLocal'), 'test'), /MIRLocal\.ssa_value: missing checked Rust field/);
const interpolation = convert('Value(MirValueId(23))', 'MIRStringPartKind');
assert.equal(variant(interpolation, 'MIRStringPartKind'), 'Interpolation');
assert.equal(variant(interpolation.fields[1], 'MIRStringFormat'), 'Display');
const imported = convert('Unqualified { module: MirModuleId(42), items: [] }', 'MIRImportKind');
assert.equal(imported.fields[0].fields[0].int, '42');
const pointerVariant = schema.get('MIROperation').variants.find(v => v.name === 'PtrFromAddr');
const pointer = { k: 'struct', name: 'PtrFromAddr', fields: pointerVariant.fields.map(f => [f.name === 'address' ? 'addr' : f.name, f.name === 'address' ? parseRustDebug('MirValueId(73)') : node(f.type)]) };
const convertedPointer = new Converter(schema).convert(pointer, named('MIROperation'), 'test');
assert.equal(variant(convertedPointer, 'MIROperation'), 'PtrFromAddr');
assert.equal(convertedPointer.fields[0].fields[0].int, '73');
assert.throws(() => new Converter(schema).convert({ ...pointer, fields: pointer.fields.filter(([key]) => key !== 'addr') }, named('MIROperation'), 'test'), /missing checked Rust field/);
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
assert.equal(decodeStream(tokens.join('\n'), schema).program.native_pack, false);
set(program, 'native_pack', parseRustDebug('true'));
const nativeTokens = ['0'];
encode(context.convert(program, named('MIRProgram'), 'test'), named('MIRProgram'), schema, nativeTokens);
assert.equal(decodeStream(nativeTokens.join('\n'), schema).program.native_pack, true);
assert.notDeepEqual(nativeTokens, tokens);
set(program, 'native_pack', parseRustDebug('false'));
assert.throws(() => decodeStream(tokens.slice(0, -1).join('\n'), schema), /truncated/);
assert.throws(() => decodeStream(tokens.concat('trailing').join('\n'), schema), /trailing/);
for (const [raw, expected] of [['None', null], ['Some(MirValueId(23))', { value: '23' }]]) {
  const bound = node(named('MIRLocal')); set(bound, 'ssa_value', parseRustDebug(raw));
  const fn = node(named('MIRFunction')); set(fn, 'locals', { k: 'list', items: [bound] });
  const observed = node(named('MIRProgram'));
  set(observed, 'schema_version', { k: 'num', v: '7' });
  set(observed, 'functions', { k: 'list', items: [fn] });
  const stream = ['0'];
  encode(new Converter(schema).convert(observed, named('MIRProgram'), 'test'), named('MIRProgram'), schema, stream);
  const decoded = decodeStream(stream.join('\n'), schema).program;
  assert.equal(decoded.schema_version, '7');
  assert.deepEqual(decoded.functions[0].locals[0].ssa_value, expected);
}
const writer = jetWriter(schema);
assert.match(writer, /fn mirw_mir_program\(/);
assert.match(writer, /mirw_bool\(v\.native_pack, &out\)/);
assert.match(writer, /mirw_mir_copy_fact\(v1, &out\)/);
assert.match(writer, /\.PtrFromAddr\(v0, v1\)/);
// Shape-only transport coverage, not valid executable MIR: every operation
// variant must survive the schema stream and have a generated Jet writer arm.
function blank(type) {
  if (type.k === 'opt') return { opt: null };
  if (type.k === 'list') return [];
  if (type.k === 'map') return { map: [] };
  if (type.name === 'String') return '';
  if (type.name === 'Bool') return false;
  if (['Int','U64','U8','U16','U32','I8','I16','I32','I64','Float','Char'].includes(type.name)) return { int: '0' };
  const def = schema.get(type.name);
  return def.kind === 'struct' ? { fields: def.fields.map(f => blank(f.type)) } : { tag: 0, fields: def.variants[0].fields.map(f => blank(f.type)) };
}
const operations = schema.get('MIROperation').variants;
const allProgram = blank(named('MIRProgram')), allFunction = blank(named('MIRFunction')), allBlock = blank(named('MIRBasicBlock'));
const replace = (value, owner, name, item) => { value.fields[schema.get(owner).fields.findIndex(f => f.name === name)] = item; };
replace(allBlock, 'MIRBasicBlock', 'instructions', operations.map((operation, tag) => {
  const instruction = blank(named('MIRInstruction'));
  replace(instruction, 'MIRInstruction', 'operation', { tag, fields: operation.fields.map(f => blank(f.type)) });
  assert.ok(writer.includes(`.${operation.name}${operation.fields.length ? '(' : ' ->'}`), `missing Jet writer arm ${operation.name}`);
  return instruction;
}));
replace(allFunction, 'MIRFunction', 'blocks', [allBlock]);
replace(allProgram, 'MIRProgram', 'functions', [allFunction]);
const allTokens = ['0']; encode(allProgram, named('MIRProgram'), schema, allTokens);
const observedOperations = decodeStream(allTokens.join('\n'), schema).program.functions[0].blocks[0].instructions.map(instruction => instruction.operation.$variant);
assert.deepEqual(observedOperations, operations.map(operation => operation.name));
const observedProgram = decodeStream(allTokens.join('\n'), schema).program;
assert.equal(observedProgram.functions[0].blocks[0].instructions.find(instruction => instruction.operation.$variant === 'EnumIs').operation.variant, '');
console.log(`front-feed codec: checked schema mappings (including PtrFromAddr), ${operations.length} operation variants and strict observation passed`);
