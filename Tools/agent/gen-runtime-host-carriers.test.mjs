import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, copyFileSync, statSync, utimesSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const script = 'Tools/agent/gen-runtime-host-carriers.mjs';
const rows = 'Compiler/JetFoundation/Source/Registry/CoreEnumPayloadRows.jet';
const outputs = ['Compiler/JetRuntime/Source/HostEnums.jet', 'crates/jet-codegen/src/Prelude/Core/CAbiHost.rs'];
function fixture(body) {
  const tree = mkdtempSync(join(tmpdir(), 'jet-host-carriers-'));
  try {
    for (const path of [script, rows, ...outputs]) {
      mkdirSync(dirname(join(tree, path)), { recursive: true });
      copyFileSync(join(root, path), join(tree, path));
    }
    const run = (...args) => spawnSync(process.execPath, [join(tree, script), ...args], { encoding: 'utf8' });
    body(tree, run);
  } finally { rmSync(tree, { recursive: true, force: true }); }
}
function success(result) { assert.equal(result.status, 0, result.stderr); assert.equal(result.signal, null); }

test('unchanged canonical outputs retain bytes and timestamps', () => fixture((tree, run) => {
  success(run());
  const old = new Date('2001-01-01T00:00:00Z');
  for (const path of outputs) utimesSync(join(tree, path), old, old);
  const before = outputs.map(path => ({ bytes: readFileSync(join(tree, path)), mtime: statSync(join(tree, path)).mtimeMs }));
  success(run());
  success(run('--check'));
  outputs.forEach((path, index) => {
    assert.deepEqual(readFileSync(join(tree, path)), before[index].bytes);
    assert.equal(statSync(join(tree, path)).mtimeMs, before[index].mtime);
  });
}));

test('check rejects either stale output without rewriting either file', () => {
  for (const stale of outputs) fixture((tree, run) => {
    success(run());
    writeFileSync(join(tree, stale), 'stale\n');
    const before = outputs.map(path => readFileSync(join(tree, path)));
    const result = run('--check');
    assert.notEqual(result.status, 0);
    assert.ok(result.stderr.includes(`${stale}: stale host carrier output`));
    outputs.forEach((path, index) => assert.deepEqual(readFileSync(join(tree, path)), before[index]));
  });
});

test('regeneration replaces only changed bytes and preserves the other output', () => {
  for (const stale of outputs) fixture((tree, run) => {
    success(run());
    const canonical = outputs.map(path => readFileSync(join(tree, path)));
    const unchanged = outputs.find(path => path !== stale);
    const old = new Date('2001-01-01T00:00:00Z');
    utimesSync(join(tree, unchanged), old, old);
    writeFileSync(join(tree, stale), 'stale\n');
    success(run());
    outputs.forEach((path, index) => assert.deepEqual(readFileSync(join(tree, path)), canonical[index]));
    assert.equal(statSync(join(tree, unchanged)).mtimeMs, old.getTime());
  });
});

// The Root source packet is authorized to change HostEnums, not CAbiHost.
// Compute both outputs in the fixture before touching the real checkout.
test('Root carrier regeneration leaves the selected Rust output byte-identical', () => fixture((tree, run) => {
  success(run());
  assert.deepEqual(readFileSync(join(tree, outputs[1])), readFileSync(join(root, outputs[1])));
}));

test('generated Jet helpers have one declaration per function', () => fixture((tree, run) => {
  success(run());
  const source = readFileSync(join(tree, outputs[0]), 'utf8');
  const names = [...source.matchAll(/\bfn\s+([A-Za-z_]\w*)\s*\(/g)].map(match => match[1]);
  assert.equal(new Set(names).size, names.length);
  assert.equal(names.filter(name => name === 'host_arguments_refused').length, 1);
}));

test('generated Rust retains the TaskFailure16 carrier and paired ownership helpers', () => fixture((tree, run) => {
  success(run());
  const source = readFileSync(join(tree, outputs[1]), 'utf8');
  assert.match(source, /struct JetCTaskFailure \{\s*pub\(crate\) tag: i64,\s*pub\(crate\) reason: JetCString,\s*\}/);
  assert.match(source, /jet_rt_alloc\(16, 8\)/);
  assert.match(source, /jet_rt_free\(pointer\.cast\(\), 16, 8\)/);
  for (const helper of ['failure_handle', 'failure_view', 'failure_drop', 'failure_take']) {
    assert.equal([...source.matchAll(new RegExp(`fn ${helper}\\(`, 'g'))].length, 1);
  }
}));
