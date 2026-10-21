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

test('borrowed IP and socket projections use allocation-free canonical value carriers', () => fixture((tree, run) => {
  success(run());
  success(run('--check'));
  const source = readFileSync(join(tree, outputs[1]), 'utf8');
  for (const [stem, type, carrier] of [
    ['ip', 'JetIpAddr', 'JetCIpAddr'],
    ['socket', 'JetSocketAddr', 'JetCSocketAddr'],
  ]) {
    const signature = new RegExp(`fn ${stem}_borrow_carrier\\(value: &crate::${type}\\) -> ${carrier} \\{`);
    const start = source.search(signature);
    assert.ok(start >= 0, `missing borrowed ${stem} conversion`);
    let depth = 1;
    let end = source.indexOf('{', start) + 1;
    const begin = end;
    for (; depth > 0 && end < source.length; end++) {
      if (source[end] === '{') depth++;
      if (source[end] === '}') depth--;
    }
    assert.equal(depth, 0);
    const body = source.slice(begin, end - 1);
    assert.doesNotMatch(body, /alloc|Box::|clone\(|drop\(|free\(|take\(|_handle\(/);
  }
  assert.match(source, /socket_carrier\(value: crate::JetSocketAddr\)[\s\S]*?socket_borrow_carrier\(&value\)/);
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
