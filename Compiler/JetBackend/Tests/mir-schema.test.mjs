import assert from 'node:assert/strict';
import test from 'node:test';
import { parseJetSchema } from './mir-debug.mjs';
import { jetWriter } from './front-feed-codec.mjs';

test('MIR schema follows public-by-default declarations and reads private fields', () => {
    const schema = parseJetSchema(`
enum MIRChoice { Empty Number(value: Int) }
struct MIRProgram {
    choice: MIRChoice
    priv count: Int
    ready: Bool{false}
}
`);
    assert.deepEqual(schema.get('MIRProgram').fields.map((field) => field.name), ['choice', 'count', 'ready']);
    assert.deepEqual(schema.get('MIRChoice').variants.map((variant) => variant.name), ['Empty', 'Number']);
    const writer = jetWriter(schema);
    assert.match(writer, /mirw_mir_choice\(v\.choice/);
    assert.match(writer, /mirw_int\(v\.count/);
    assert.match(writer, /mirw_bool\(v\.ready/);
});

test('retired public modifiers do not become a parallel schema grammar', () => {
    const schema = parseJetSchema('pub struct MIRProgram { pub count: Int }');
    assert.equal(schema.has('MIRProgram'), false);
    assert.throws(() => jetWriter(schema), /missing MIR schema type MIRProgram/);
});
