import test from 'node:test';
import assert from 'node:assert/strict';
import { functionsIn, checkOriginal } from './test-port.mjs';

const fn = (body) => functionsIn(`#[test] fn example() { ${body} }`, 'tests/sample.rs')[0];
const source = 'fn run() {}';
const body = `let diags = jet::compile("${source}").expect_err("rejected"); assert_eq!(diags.len(), 1);`;
const reference = async (source, mode) => ({ source, mode, code: 1, diagnostics: [{ code: 'E0209' }] });

test('only exact negative Rust availability early-return guards are ignored', async () => {
  for (const name of ['have_rustc', 'common::have_rustc']) {
    const checked = await checkOriginal(fn(`if !${name}() { return; } ${body}`), reference);
    assert.equal(checked.observations.length, 1);
    assert.equal(checked.assertions, 2);
    await assert.rejects(checkOriginal(fn(`if !${name}() { return; } ${body} assert!(false);`), reference), /not satisfied/u);
  }
});

test('planted nontrivial availability guards never discard assertions or branches', async () => {
  for (const guard of [
    'if have_rustc() { return; }',
    'if !have_rustc() { assert!(false); return; }',
    'if !have_rustc() { return; } else { assert!(false); }',
    'if !have_rustc() && true { return; }',
    'if !other::have_rustc() { return; }',
    'if !have_rustc() { return 1; }',
  ]) await assert.rejects(checkOriginal(fn(`${guard} ${body}`), reference));
});
