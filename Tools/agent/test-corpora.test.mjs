import assert from 'node:assert/strict';
import test from 'node:test';
import { readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { goldenVerdict, normalizeUi, renderedOnly, snapshotVerdict, uiEntries, uiInvocation } from './test-corpora.mjs';
import { rustCode, inventory } from './test-harness-inventory.mjs';
import { safeGolden, stripVetted } from './golden-safety.mjs';

const entry = { stem: 'basics/hello', path: '/source/hello.jet' };
const expected = { out: 'hello\n', err: null, stderrOut: null };
const run = { code: 0, stdout: 'hello\n', stderr: '' };

test('goldens preserve every byte, including trailing newline and CR', () => {
  assert.equal(goldenVerdict(entry, run, expected, '/bin/hello').status, 'PASS');
  for (const stdout of ['hello', 'hello\n\n', 'hello\r\n']) assert.equal(goldenVerdict(entry, { ...run, stdout }, expected, '/bin/hello').status, 'FAIL');
});
test('a warning cannot turn a failed process into success', () => {
  assert.equal(goldenVerdict(entry, { ...run, code: 1, stderr: 'Warning [L0507]: warning\n' }, expected, '/bin/hello').status, 'FAIL');
});
test('expected failure requires the compiler-diff exit code and exact stdout/stderr', () => {
  const want = { out: null, err: 'Stop [E3001]: panic\n', stderrOut: null };
  assert.equal(goldenVerdict(entry, { code: 70, stdout: '', stderr: want.err }, want, '/bin/hello').status, 'PASS');
  for (const code of [0, 1, 2, 101, null]) assert.equal(goldenVerdict(entry, { code, stdout: '', stderr: want.err }, want, '/bin/hello').status, 'FAIL');
  assert.equal(goldenVerdict(entry, { code: 70, stdout: 'ignored', stderr: want.err }, want, '/bin/hello').status, 'FAIL');
  assert.equal(goldenVerdict(entry, { code: 70, stdout: '', stderr: 'Stop [E3001]: other\n' }, want, '/bin/hello').status, 'FAIL');
  assert.equal(goldenVerdict(entry, { code: 1, stdout: '', stderr: 'unknown report\n' }, { out: null, err: 'unknown report\n', stderrOut: null }, '/bin/hello').status, 'FAIL');
});
test('success auxiliary stderr is exact and mandatory when pinned', () => {
  assert.equal(goldenVerdict(entry, run, { ...expected, stderrOut: 'note\n' }, '/bin/hello').status, 'FAIL');
});
test('CLI argv0 normalization replaces only the first Usage occurrence', () => {
  assert.equal(goldenVerdict({ ...entry, stem: 'cli/hello' }, { ...run, stdout: 'Usage: binary\nUsage: binary\n' }, { ...expected, out: 'Usage: hello\nUsage: binary\n' }, '/bin/binary').status, 'PASS');
});
test('compute availability compares the Rust portable contract', () => {
  const compute = { ...entry, stem: 'tooling/compute_vulkan_webgpu' };
  for (const availability of ['accepted', 'rejected']) assert.equal(goldenVerdict(compute, { ...run, stdout: `vulkan:${availability}\nwebgpu:rejected\n` }, expected, '/bin/c').status, 'PASS');
  assert.equal(goldenVerdict(compute, { ...run, stdout: 'vulkan:accepted\nwebgpu:accepted\n' }, expected, '/bin/c').status, 'FAIL');
});
test('API-only and unknown UI directives fail closed', () => {
  assert.match(uiInvocation('// @jetlib_effect_refused\n', 'tests/ui/a.jet').refused, /jetlib_effect_refused/u);
  assert.match(uiInvocation('// @new_internal_mode\n', 'tests/ui/a.jet').refused, /new_internal_mode/u);
  assert.deepEqual(uiInvocation('// ordinary comment\n', 'tests/ui/a.jet').args, ['check', 'tests/ui/a.jet']);
});
test('real check CLI fixtures preserve their basename/cwd contract', () => {
  const plan = uiInvocation('// @check_cli\n', 'tests/ui/project/run.jet');
  assert.deepEqual(plan.args, ['check', 'run.jet']);
  assert.equal(plan.cwdParent, true);
  assert.equal(plan.direct, true);
});
test('internal snapshots discard only the known CLI envelope', () => {
  const rendered = 'Error [E0109]: message\n Why: 1 problem found\nMore: jet-lang.dev/e/E0109\n';
  assert.equal(renderedOnly(rendered + '\n1 problem found\nrun `jet explain E0109` to learn more\n'), rendered);
  assert.equal(renderedOnly(rendered), rendered);
});
test('UI normalizer matches Rust lines and repo/workspace normalization', () => {
  assert.equal(normalizeUi('tests/ui/a/workspace.jet', '/repo/tests/ui/a/file\r\nBlocking waiting for file lock on package cache\n', '/repo'), '/tmp/file\n');
});
test('primary UI header paths normalize symmetrically without hiding content differences', () => {
  const entry = { suite: 'ui', shown: 'tests/ui/example.jet' };
  const short = 'Error [E0109]: mismatch\n  --> example.jet:2:4\n Why: example.jet\n  --> imported.jet:8:3\n';
  const canonical = short.replace('  --> example.jet:', '  --> tests/ui/example.jet:');
  assert.equal(snapshotVerdict(entry, run, short, canonical).status, 'PASS');
  assert.equal(snapshotVerdict(entry, run, canonical, short).status, 'PASS');
  for (const changed of [short.replace('E0109', 'E0107'), short.replace('mismatch', 'different'), short.replace(':2:4', ':2:5'), short.replace('Why: example.jet', 'Why: tests/ui/example.jet'), short.replace('imported.jet', 'elsewhere.jet')]) assert.equal(snapshotVerdict(entry, run, changed, canonical).status, 'FAIL');
});
test('missing snapshots and empty lint output cannot pass', () => {
  assert.equal(snapshotVerdict({ suite: 'ui' }, run, '', null).status, 'FAIL');
  assert.equal(snapshotVerdict({ suite: 'lint' }, run, '', '').status, 'FAIL');
});
test('UI discovery excludes fixed sources and chooses canonical directory entries', () => {
  const entries = uiEntries();
  assert.ok(entries.length > 0);
  assert.ok(entries.every((e) => !e.shown.includes('.fixed.')));
  assert.ok(entries.every((e) => e.expected.endsWith('.stderr') || e.expected.endsWith('/stderr')));
});
test('Rust census ignores embedded programs, nested comments and raw strings', () => {
  const code = rustCode('jet::compile(); /* /* jetpack::fake() */ comment */ let a = r###"jet_sema::fake()"###; let b = "jet::fake()"; let c: &\'a str;');
  assert.match(code, /jet::compile/u);
  assert.doesNotMatch(code, /fake|comment/u);
  assert.match(code, /&'a str/u);
});
test('inventory is complete, unique, and retains the mixed golden/UI targets', () => {
  const rows = inventory();
  const files = (directory) => readdirSync(directory, { withFileTypes: true }).flatMap((item) => item.isDirectory() ? files(`${directory}/${item.name}`) : item.isFile() && item.name.endsWith('.rs') ? [`${directory}/${item.name}`] : []);
  const root = fileURLToPath(new URL('../../', import.meta.url)).replace(/\/$/u, '');
  assert.deepEqual(rows.map((r) => r.file), files(`${root}/tests`).map((file) => file.slice(root.length + 1)).sort());
  assert.equal(new Set(rows.map((r) => r.file)).size, rows.length);
  for (const file of ['tests/golden.rs', 'tests/diagnostic_snapshots.rs']) {
    const row = rows.find((r) => r.file === file);
    assert.equal(row.class, 'c');
    assert.match(row.dependencies, /jet/u);
  }
});

test('I1 removes only registered modules/regions and reads keywords as code', () => {
  const fixture = 'mod jet_mem { /* outer /* nested */ end */ unsafe { ffi() } }\nfn main() { print(\"unsafe\"); }\n';
  assert.equal(safeGolden(fixture, 'basics/hello'), null);
  assert.match(safeGolden(fixture + 'fn user() { unsafe { pointer() } }\n', 'basics/hello'), /I1/u);
  const marker = '// JET_VETTED_UNSAFE_BEGIN: jet_watch_process_probe\nunsafe { ffi() }\n// JET_VETTED_UNSAFE_END: jet_watch_process_probe\nfn main() {}\nunsafe { user() }\n';
  const stripped = stripVetted(marker);
  assert.doesNotMatch(stripped, /ffi/u);
  assert.match(stripped, /user/u);
});
