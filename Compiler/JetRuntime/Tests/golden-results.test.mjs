import assert from 'node:assert/strict';
import {mkdtempSync, rmSync, writeFileSync} from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import {readGoldenResults} from './golden-results.mjs';

function receipt(text, body) {
  const directory = mkdtempSync(path.join(os.tmpdir(), 'jet-golden-results-'));
  const file = path.join(directory, 'results.tsv');
  try { writeFileSync(file, text); body(file); }
  finally { rmSync(directory, {recursive: true, force: true}); }
}
test('a shared union retains per-stem actual verdicts and detail columns', () => {
  receipt('math/int_div\tpass\tnative\ntext/unicode_17\twrong\tstderr differs\n', file => {
    assert.deepEqual([...readGoldenResults(file)], [['math/int_div', 'pass'], ['text/unicode_17', 'wrong']]);
    assert.equal(readGoldenResults(file).get('not-in-corpus'), undefined);
  });
});
test('duplicate results cannot discharge a native corpus', () => {
  receipt('math/int_div\twrong\nmath/int_div\tpass\n', file => assert.throws(() => readGoldenResults(file), /duplicate/));
});
test('empty and malformed evidence fails closed', () => {
  for (const text of ['', '\n', 'math/int_div\n', '\tpass\n', 'math/int_div\t\n']) {
    receipt(text, file => assert.throws(() => readGoldenResults(file), /empty|malformed/));
  }
});
