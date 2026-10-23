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
const outputs = ['Compiler/JetRuntime/Source/HostEnums.jet'];
function fixture(body) {
  const tree = mkdtempSync(join(tmpdir(), 'jet-host-carriers-'));
  try {
    // Preserve the generator's real module boundary in the isolated tree.
    for (const path of [script, 'Tools/agent/runtime-abi-symbols.mjs', rows, ...outputs]) {
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

test('check rejects a stale output without rewriting it', () => fixture((tree, run) => {
  const [stale] = outputs;
  success(run());
  writeFileSync(join(tree, stale), 'stale\n');
  const result = run('--check');
  assert.notEqual(result.status, 0);
  assert.ok(result.stderr.includes(`${stale}: stale host carrier output`));
  assert.equal(readFileSync(join(tree, stale), 'utf8'), 'stale\n');
}));

test('regeneration restores the canonical output bytes', () => fixture((tree, run) => {
  const [stale] = outputs;
  success(run());
  const canonical = readFileSync(join(tree, stale));
  writeFileSync(join(tree, stale), 'stale\n');
  success(run());
  assert.deepEqual(readFileSync(join(tree, stale)), canonical);
}));

test('generated Jet helpers have one declaration per function', () => fixture((tree, run) => {
  success(run());
  const source = readFileSync(join(tree, outputs[0]), 'utf8');
  const names = [...source.matchAll(/\bfn\s+([A-Za-z_]\w*)\s*\(/g)].map(match => match[1]);
  assert.equal(new Set(names).size, names.length);
  assert.equal(names.filter(name => name === 'host_arguments_refused').length, 1);
}));

test('borrowed C-field lowering supplies stack storage and retains its borrow flag', () => {
  const lower = readFileSync(join(root, 'Compiler/JetBackend/Source/Lower/Places.jet'), 'utf8');
  const body = lower.slice(lower.indexOf('fn lower_handle_field_place('), lower.indexOf('fn lower_projected_move_supported('));
  assert.match(body, /lent\.header_bytes > 0/);
  assert.match(body, /StackSlot\{dst: address, size: bytes, align: 8\}/);
  assert.match(body, /\[LIRValueID\]\{handle, address\}, None/);
  assert.match(body, /ty: field\.ty, borrowed: true/);
  assert.doesNotMatch(body, /lower_alloc\(/);
});
