import assert from 'node:assert/strict';
import test from 'node:test';
import {rustDataEventVariantFamily} from './check-core-surface-ledger.mjs';

const constants = {strings: new Map(), lists: new Map()};
const source = `
pub const DATA_EVENT_VARIANTS: [&str; 3] = ["Text", "Null", "Key"];
fn data_event_payload(variant: &str) -> Option<Option<Type>> {
    match variant {
        "Null" => Some(None),
        "Text" | "Key" => Some(Some(Type::String)),
        _ => None,
    }
}
fn core_encoding_variants(enum_name: &str) {
    for name in DATA_EVENT_VARIANTS {
        let payload = match data_event_payload(name) {
            Some(Some(ty)) => VariantPayload::Single(ty, zero),
            _ => VariantPayload::Unit,
        };
    }
}
`;
test('DataEvent projection uses the canonical table order and grouped payload arms', () => {
    assert.deepEqual(rustDataEventVariantFamily(source, constants), {
        name: 'DataEvent', rows: [
            {variant: 'Text', payload: ['Type.String']},
            {variant: 'Null', payload: []},
            {variant: 'Key', payload: ['Type.String']},
        ],
    });
});
test('missing payload metadata is a loud error, not a unit fallback', () => {
    assert.throws(() => rustDataEventVariantFamily(source.replace('"Text" | "Key" =>', '"Text" =>'), constants), /payload projection is incomplete/);
});
test('duplicate canonical variants fail loudly', () => {
    assert.throws(() => rustDataEventVariantFamily(source.replace('["Text", "Null", "Key"]', '["Text", "Null", "Text"]'), constants), /incomplete or duplicated/);
});
test('unrecognized payload arms fail loudly', () => {
    assert.throws(() => rustDataEventVariantFamily(source.replace('Some(Some(Type::String))', 'unknown_payload()'), constants), /unrecognized DataEvent payload arm/);
});
