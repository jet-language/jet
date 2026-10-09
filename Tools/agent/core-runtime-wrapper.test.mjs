import assert from 'node:assert/strict';
import test from 'node:test';
import {ownedWrapperRuntimeUnit} from './check-core-surface-ledger.mjs';

const importUnit = 'use core.text.runtime.copy as kernel\n';
test('default-public function wrappers expose their runtime provider', () => {
    assert.equal(ownedWrapperRuntimeUnit(importUnit + 'fn copy() -> Int { kernel.copy() }\n', 'core.text', 'copy'), 'core.text.runtime.copy');
});
test('current declaration annotations do not hide runtime wrappers', () => {
    assert.equal(ownedWrapperRuntimeUnit(importUnit + '#Unsafe("validated carrier") fn copy() -> Int { kernel.copy() }\n', 'core.text', 'copy'), 'core.text.runtime.copy');
});
test('selective member aliases expose the same runtime provider', () => {
    const source = 'use core.text.runtime.copy.[copy as raw_copy]\nfn copy() -> Int { raw_copy() }\n';
    assert.equal(ownedWrapperRuntimeUnit(source, 'core.text', 'copy'), 'core.text.runtime.copy');
});
test('a compiler-owned provider call is not a runtime cutover', () => {
    assert.equal(ownedWrapperRuntimeUnit(importUnit + 'fn copy() -> Int { core.text.copy() }\n', 'core.text', 'copy'), null);
});
test('the next module is outside the wrapper body', () => {
    const source = importUnit + 'fn copy() -> Int { 0 }\nmodule internal {\n    fn helper() -> Int { kernel.copy() }\n}\n';
    assert.equal(ownedWrapperRuntimeUnit(source, 'core.text', 'copy'), null);
});
test('retired pub declaration spelling is not a legacy reader path', () => {
    assert.equal(ownedWrapperRuntimeUnit(importUnit + 'pub fn copy() -> Int { kernel.copy() }\n', 'core.text', 'copy'), null);
});
