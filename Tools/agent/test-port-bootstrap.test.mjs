import test from 'node:test';
import assert from 'node:assert/strict';
import { functionsIn, checkOriginal } from './test-port.mjs';

const fn = (body) => functionsIn(`use common::have_rustc; #[test] fn example() { ${body} }`, 'tests/sample.rs')[0];
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

test('E1 imported availability aliases, literal skip notes and exact local skip helper', async () => {
  const rows = functionsIn(`use common::have_rustc as available;
    fn skip_without_rustc() -> bool { !available() }
    #[test] fn example() {
      if skip_without_rustc() { eprintln!("need rustc"); return; }
      if !available() { eprintln!("need rustc"); return }
      ${body}
    }`, 'tests/sample.rs');
  const row = { ...rows.find(row => row.test), helpers: rows.filter(row => !row.test) };
  const checked = await checkOriginal(row, reference);
  assert.equal(checked.assertions, 2);
});

test('E1 near-miss helpers and side-effectful notes stay hand-port', async () => {
  for (const helperBody of ['true', '!common::have_rustc() || true', 'assert!(true); !common::have_rustc()', '!other::have_rustc()']) {
    const rows = functionsIn(`fn skip_without_rustc() -> bool { ${helperBody} }
      #[test] fn example() { if skip_without_rustc() { return; } ${body} }`, 'tests/sample.rs');
    await assert.rejects(checkOriginal({ ...rows[1], helpers: [rows[0]] }, reference));
  }
  for (const guard of [
    'if !have_rustc() { eprintln!("{}", assert!(false)); return; }',
    'if !have_rustc() { eprintln!("note"); assert!(false); return; }',
  ]) await assert.rejects(checkOriginal(fn(`${guard} ${body}`), reference));
  const rows = functionsIn(`fn have_rustc() -> bool { false }
    #[test] fn example() { if !have_rustc() { return; } ${body} }`, 'tests/sample.rs');
  await assert.rejects(checkOriginal({ ...rows[1], helpers: [rows[0]] }, reference));
});

test('E1b immutable availability binding used only by an adjacent negative guard', async () => {
  for (const note of ['', 'eprintln!("need rustc");']) {
    const checked = await checkOriginal(fn(`let present = have_rustc(); if !present { ${note} return; } ${body}`), reference);
    assert.equal(checked.assertions, 2);
    await assert.rejects(checkOriginal(fn(`let present = have_rustc(); if !present { ${note} return; } ${body} assert!(false);`), reference), /not satisfied/u);
  }
});

test('E1b later uses, mutation, reassignment and changed conditions stay hand-port', async () => {
  for (const setup of [
    'let present = have_rustc(); if !present { return; } assert!(present);',
    'let mut present = have_rustc(); if !present { return; }',
    'let present = have_rustc(); present = false; if !present { return; }',
    'let present = have_rustc(); if present { return; }',
    'let present = have_rustc(); if !present && true { return; }',
  ]) await assert.rejects(checkOriginal(fn(`${setup} ${body}`), reference));
  const unrelated = functionsIn(`fn another() { use common::have_rustc; }
    #[test] fn example() { if !have_rustc() { return; } ${body} }`, 'tests/sample.rs')[1];
  await assert.rejects(checkOriginal(unrelated, reference));
});
