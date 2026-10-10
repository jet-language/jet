import assert from 'node:assert/strict';
import test from 'node:test';
import { partitionRegistries } from './registry-shard.mjs';

const manifest = ['Z', 'A', 'T', 'B', 'Q', 'C', 'P'].map(id => ({ id }));
test('unsharded keeps canonical manifest order and identity', () => {
    assert.equal(partitionRegistries(manifest), manifest);
});
test('shards are deterministic, disjoint and cover the complete manifest', () => {
    for (const count of [1, 2, 3, 10]) {
        const all = Array.from({ length: count }, (_, index) => partitionRegistries(manifest, `${index + 1}/${count}`)).flat();
        assert.equal(all.length, manifest.length);
        assert.equal(new Set(all.map(row => row.id)).size, manifest.length);
        assert.deepEqual(all.map(row => row.id).sort(), manifest.map(row => row.id).sort());
        for (let index = 1; index <= count; index++) {
            assert.deepEqual(partitionRegistries([...manifest].reverse(), `${index}/${count}`), partitionRegistries(manifest, `${index}/${count}`));
        }
    }
});
test('invalid shards cannot accidentally run a partial manifest', () => {
    for (const shard of ['0/2', '3/2', '1/0', '-1/2', '1', '1/2/3', '1.5/2', '1/9007199254740992']) {
        assert.throws(() => partitionRegistries(manifest, shard), /invalid registry shard/);
    }
});
