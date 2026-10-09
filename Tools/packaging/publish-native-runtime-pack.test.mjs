import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { captureStep, captureTerminal, fileObject, lifecycleSection, main, observeStep, parseProofqMetadata, publication, readelfSectionNames } from './publish-native-runtime-pack.mjs';

function fixture(fn) {
  const root = mkdtempSync(join(tmpdir(), 'native-runtime-publication-'));
  try { fn(root); } finally { rmSync(root, { recursive: true, force: true }); }
}
function put(path, value) { writeFileSync(path, JSON.stringify(value)); }

test('ObjectHandle uses actual held bytes and exact decimal string', () => fixture(root => {
  const path = join(root, 'bytes'); writeFileSync(path, 'é\n');
  const handle = fileObject(path);
  assert.match(handle, /^[0-9a-f]{64}:3$/);
  writeFileSync(path, 'different'); assert.notEqual(fileObject(path), handle);
}));

test('capture preserves actual overwritten inputs, observe binds original bytes and real log', () => fixture(root => {
  const invocation = join(root, 'command'), input = join(root, 'input'), descriptor = join(root, 'descriptor.json');
  writeFileSync(invocation, 'node actual-script.mjs actual-input\n'); writeFileSync(input, 'original bytes');
  put(descriptor, { schema: 'jet.native-runtime-step-capture/v1', id: 'one-small-step', invocation, inputs: [{ role: 'source-é', path: input }] });
  const capturedPath = captureStep(descriptor, join(root, 'capture'));
  const captured = JSON.parse(readFileSync(capturedPath, 'utf8'));
  assert.equal(captured.inputs[0].object, fileObject(input));
  writeFileSync(input, 'overwritten producer input');
  assert.equal(fileObject(captured.files[1].path), captured.inputs[0].object);
  const output = join(root, 'output'), log = join(root, 'stdout'), outputs = join(root, 'outputs.json');
  writeFileSync(output, 'real small output'); writeFileSync(log, 'actual small-step output\n');
  put(outputs, [{ role: 'output', path: output }]);
  const receipt = observeStep(capturedPath, '0', outputs, log, join(root, 'observation.json'));
  assert.equal(receipt.id, 'one-small-step'); assert.equal(receipt.rc, '0');
  assert.equal(receipt.invocation, captured.invocation); assert.deepEqual(receipt.inputs, captured.inputs);
  assert.equal(receipt.outputs[0].object, fileObject(output)); assert.equal(receipt.log, fileObject(log));
  assert.throws(() => observeStep(capturedPath, '0', outputs, log, join(root, 'observation.json')), /EEXIST/);
}));

test('capture rejects duplicate roles and cannot overwrite evidence', () => fixture(root => {
  const input = join(root, 'input'), invocation = join(root, 'invocation'), descriptor = join(root, 'descriptor');
  writeFileSync(input, 'input'); writeFileSync(invocation, 'invocation');
  put(descriptor, { schema: 'jet.native-runtime-step-capture/v1', id: 'step', invocation, inputs: [{ role: 'same', path: input }, { role: 'same', path: input }] });
  assert.throws(() => captureStep(descriptor, join(root, 'capture')), /duplicate role/);
  assert.throws(() => captureStep(descriptor, join(root, 'capture')), /overwrite/);
}));

test('observation preserves nonzero rc and rejects rounded/negative status', () => fixture(root => {
  const invocation = join(root, 'invocation'), descriptor = join(root, 'descriptor'), outputs = join(root, 'outputs'), log = join(root, 'log');
  writeFileSync(invocation, 'real negative invocation'); writeFileSync(log, 'actual exit 101\n'); put(outputs, []);
  put(descriptor, { schema: 'jet.native-runtime-step-capture/v1', id: 'raw-frontend', invocation, inputs: [] });
  const captured = captureStep(descriptor, join(root, 'capture'));
  assert.equal(observeStep(captured, '101', outputs, log, join(root, 'observation')).rc, '101');
  for (const status of ['-1', '01', '1.0', '256', 0]) assert.throws(() => observeStep(captured, status, outputs, log, join(root, 'invalid')), /invalid process rc/);
}));

test('red or nonterminal done refuses before publisher or candidate evidence exists', () => fixture(root => {
  const done = join(root, 'done'), manifest = join(root, 'manifest');
  const values = { schema: 'jet.native-runtime-publication-input/v1', worktree: root, done, job: '', admission: '', rail_contract: '', compiler: '', generation: '', raw_frontend: '', final_source_tree: '', steps_tsv: '', observations: [], artifacts: {}, roles: {} };
  put(manifest, values);
  for (const status of ['1\n', '125\n', '', '0', 'running\n']) {
    writeFileSync(done, status);
    assert.throws(() => publication('prepare', manifest, join(root, 'nonexistent-publisher'), join(root, 'publication')), /actual terminal proofq/);
  }
  assert.throws(() => publication('publish', manifest, 'missing', 'unused'), /explicit subject-bound authorization/);
  assert.throws(() => publication('prepare', manifest, 'missing', 'unused', 'unexpected'), /prepare forbids/);
  assert.throws(() => main(['unknown']), /internal publication tool/);
}));

test('proofq metadata requires actual terminal name/time/status without inventing absent metrics', () => {
  const sparse = 'name=fixture\nrc=0\nend=11\nstart=10\nkind=test\n';
  const parsed = parseProofqMetadata(Buffer.from(sparse), 'fixture');
  assert.equal(parsed.get('end'), '11'); assert.equal(parsed.has('peak'), false);
  const complete = `${sparse}mem=24\npeak=\ntime=120\ntimeout_min=180\ndeclared=1\nseen=9\nqueued_mtime=8\navail_at_start=49.6\nheader_warnings=\npeak_kb=47568\nrun_s=1\n`;
  assert.equal(parseProofqMetadata(Buffer.from(complete), 'fixture').get('peak_kb'), '47568');
  for (const invalid of [sparse + 'rc=0\n', sparse.replace('rc=0', 'rc=125'), sparse.replace('fixture', 'other'), sparse.replace('end=11', 'end=9'), sparse.replace('end=11\n', ''), sparse.replace('start=10', 'start=01'), sparse.replace('kind=test', 'kind=unknown'), '=value\n' + sparse, sparse + 'malformed\n']) {
    assert.throws(() => parseProofqMetadata(Buffer.from(invalid), 'fixture'));
  }
  assert.throws(() => parseProofqMetadata(Buffer.from([255]), 'fixture'));
});

test('terminal envelope binds original script/status/metadata bytes (schema fixture, not execution proof)', () => fixture(root => {
  const name = 'fixture', done = join(root, `${name}.rc`), job = join(root, `${name}.sh`), metadata = join(root, `${name}.meta`), rail = join(root, 'rail.json');
  writeFileSync(done, '0\n'); writeFileSync(job, 'unit fixture job bytes, not an executed E2 job\n');
  writeFileSync(metadata, `name=${name}\nrc=0\nstart=10\nend=11\nkind=test\n`);
  const contract = { proofq_name: name, job: fileObject(job) }; put(rail, contract);
  const subject = { runtime_source_revision: '0'.repeat(40), runtime_source_tree: '1'.repeat(40) };
  const manifest = { done, job, rail_contract: rail };
  const handle = captureTerminal(manifest, contract, subject, fileObject, root);
  const path = join(root, 'proofq-terminal.json'), receipt = JSON.parse(readFileSync(path, 'utf8'));
  assert.equal(handle, fileObject(path)); assert.equal(receipt.job, fileObject(job));
  assert.equal(receipt.done, fileObject(done)); assert.equal(receipt.proofq_metadata, fileObject(metadata));
  assert.equal(receipt.rail_contract, fileObject(rail)); assert.equal(receipt.runtime_source_tree, subject.runtime_source_tree);
  assert.throws(() => captureTerminal({ ...manifest, done: join(root, 'other.rc') }, contract, subject, fileObject, root), /does not locate/);
  writeFileSync(job, 'changed actual bytes');
  assert.throws(() => captureTerminal(manifest, contract, subject, fileObject, root), /differs from admitted job/);
  writeFileSync(metadata, `name=${name}\nrc=0\nstart=10\nend=11\nkind=test\nrc=0\n`);
  assert.throws(() => captureTerminal(manifest, contract, subject, fileObject, root), /duplicate/);
}));

test('lifecycle section is exact five-digit canonical priority, never a padding alias', () => {
  assert.equal(lifecycleSection('init', '0'), '.init_array.00000');
  assert.equal(lifecycleSection('fini', '1'), '.fini_array.00001');
  assert.equal(lifecycleSection('init', '65535'), '.init_array.65535');
  for (const priority of ['00', '00001', '65536', '-1', '1.0', '1.extra', ' 1', '', 1]) assert.throws(() => lifecycleSection('init', priority));
  assert.throws(() => lifecycleSection('unknown', '0'));
});

test('lifecycle evidence uses exact actual readelf section-name fields', () => {
  const header = 'Section Headers:\n  [Nr] Name              Type            Address          Off    Size   ES Flg Lk Inf Al\n';
  const row = name => `  [ 3] ${name} INIT_ARRAY      0000000000000000 000040 000008 08  WA  0   0  8\n`;
  const expected = lifecycleSection('init', '0');
  assert(readelfSectionNames(header + row(expected)).has(expected));
  assert(readelfSectionNames(header + row(lifecycleSection('fini', '1'))).has('.fini_array.00001'));
  assert(readelfSectionNames(header + row(lifecycleSection('init', '65535'))).has('.init_array.65535'));
  for (const alias of ['.init_array.00000.extra', '.init_array.000000', '.init_array.0', '.init_array.000é0', '.init_array.0000٠', '.rela.init_array.00000']) {
    assert(!readelfSectionNames(header + row(alias)).has(expected), alias);
  }
  assert(!readelfSectionNames(`${header}  prose contains ${expected}\n`).has(expected));
  assert(!readelfSectionNames(`${header}  [Nr] ${expected} INIT_ARRAY\n`).has(expected));
  assert(!readelfSectionNames(header + row('.other') + `  note ${expected}\n`).has(expected));
});
