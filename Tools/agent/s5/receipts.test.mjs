import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {finalizeDriver, phaseSchema, readPrerequisite, writePhaseReceipt} from './receipts.mjs';
import {consumePrepared, preparedFiles} from './prepared.mjs';
import {readSharedPack} from './shared-publications.mjs';

const head = 'a'.repeat(40), tree = 'b'.repeat(40), key = 'c'.repeat(64);
const temporary = t => { const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'jet-s5-receipts-')); t.after(() => fs.rmSync(dir, {recursive: true, force: true})); return dir; };
const successful = () => ({schema: phaseSchema, head, phase: 'goldens', runId: 'unit', complete: true, verdict: 'PASS', steps: [{name: 'union', verdict: 'PASS'}], runners: []});
function publication(dir, receipt = successful(), rc = '0\n') {
  writePhaseReceipt(dir, receipt); fs.writeFileSync(path.join(dir, 'driver.rc'), rc);
}

test('prerequisites require real driver success, completion, pin, phase and decisive rows', t => {
  const dir = temporary(t); publication(dir);
  assert.equal(readPrerequisite(dir, head, ['union'], 'goldens').problem, null);
  assert.match(readPrerequisite(dir, head, ['missing']).problem, /decisive/);
  assert.match(readPrerequisite(dir, head, [], 'runtime').problem, /phase differs/);
  assert.match(readPrerequisite(dir, 'd'.repeat(40)).problem, /same-pin/);
  fs.writeFileSync(path.join(dir, 'driver.rc'), '42\n');
  assert.match(readPrerequisite(dir, head).problem, /real driver/);
});

test('missing, malformed, incomplete and failed receipts are blocked inputs', t => {
  const dir = temporary(t);
  assert.ok(readPrerequisite(dir, head).problem);
  fs.writeFileSync(path.join(dir, 'driver.rc'), '0\n'); fs.writeFileSync(path.join(dir, 'steps.json'), '{');
  assert.ok(readPrerequisite(dir, head).problem);
  publication(dir, {...successful(), complete: false});
  assert.ok(readPrerequisite(dir, head).problem);
  publication(dir, {...successful(), steps: [{name: 'union', verdict: 'FAIL'}]});
  assert.match(readPrerequisite(dir, head).problem, /failed/);
  publication(dir, {...successful(), steps: [{name: 'union', verdict: 'PASS'}, {name: 'union', verdict: 'PASS'}]});
  assert.match(readPrerequisite(dir, head, ['union']).problem, /decisive/);
});

test('retired oracle rows stay UNCOVERED while native goldens and AGE remain decisive', t => {
  const dir=temporary(t);
  const receipt={...successful(),phase:'fixtures',steps:[
    {name:'shared-native-results',verdict:'PASS'},
    {name:'run-crypto-fixtures-4',verdict:'PASS'},
    {name:'run-display-goldens-1',verdict:'UNCOVERED',cardRequired:true},
    {name:'freeze-age-fixtures-0',verdict:'UNCOVERED',cardRequired:true},
  ],runners:[{name:'freeze-age-fixtures.mjs',verdict:'UNCOVERED'}]};
  publication(dir,receipt);
  assert.equal(readPrerequisite(dir,head,['shared-native-results','run-crypto-fixtures-4'],'fixtures').problem,null);
  assert.match(readPrerequisite(dir,head,['freeze-age-fixtures-0'],'fixtures').problem,/not PASS/);
  for(const name of ['shared-native-results','run-crypto-fixtures-4']) {
    publication(dir,{...receipt,steps:receipt.steps.map(row=>row.name===name?{...row,verdict:'FAIL'}:row)});
    assert.match(readPrerequisite(dir,head,[],'fixtures').problem,/failed/);
  }
});

test('timeout preserves partial evidence and publishes an explicit completed failure', t => {
  const dir = temporary(t), partial = {...successful(), complete: false, verdict: 'RUNNING'};
  writePhaseReceipt(dir, partial);
  const done = finalizeDriver(dir, {head, phase: 'goldens', runId: 'unit', code: 124});
  assert.equal(done.complete, true); assert.equal(done.verdict, 'FAIL');
  assert.equal(done.steps[0].name, 'union'); assert.equal(done.steps[1].code, 124);
  assert.deepEqual(JSON.parse(fs.readFileSync(done.steps[1].partial, 'utf8')), partial);
  assert.equal(fs.readFileSync(path.join(dir, 'driver.rc'), 'utf8'), '124\n');
});

test('an old completed verdict cannot survive a new run without its receipt', t => {
  const dir = temporary(t); publication(dir);
  const done = finalizeDriver(dir, {head, phase: 'goldens', runId: 'new-run', code: 0});
  assert.equal(done.verdict, 'FAIL'); assert.equal(done.steps.length, 1);
  assert.match(done.steps[0].partial, /incomplete-new-run/);
  assert.equal(fs.readFileSync(path.join(dir, 'driver.rc'), 'utf8'), '0\n');
});

test('normal completed verdicts retain the real driver result', t => {
  const dir = temporary(t); publication(dir);
  assert.equal(finalizeDriver(dir, {head, phase: 'goldens', runId: 'unit', code: 0}).verdict, 'PASS');
  const blocked = {...successful(), verdict: 'BLOCKED', steps: [{name: 'pack', verdict: 'BLOCKED'}]};
  publication(dir, blocked, '2\n');
  assert.equal(finalizeDriver(dir, {head, phase: 'goldens', runId: 'unit', code: 2}).verdict, 'BLOCKED');
});

test('prepared corpora are copied exactly once after inventory and hash verification', t => {
  const dir = temporary(t), source = path.join(dir, 'prepared'), destination = path.join(dir, 'consumer');
  fs.mkdirSync(source); fs.writeFileSync(path.join(source, 'cases.list'), 'case\n');
  const manifest = preparedFiles(source);
  consumePrepared(source, destination, manifest);
  assert.deepEqual(preparedFiles(destination), manifest);
  assert.throws(() => consumePrepared(source, destination, manifest), /already exists/);
  fs.writeFileSync(path.join(source, 'cases.list'), 'drift\n');
  assert.throws(() => consumePrepared(source, path.join(dir, 'other'), manifest), /digest differs/);
});

function packFixture(dir) {
  // Publication-guard data, not compiled objects or a native execution receipt.
  const source = path.join(dir, 'source'), directory = path.join(dir, 'runtime-pack'), runtimeDir = path.join(directory, 'jet-runtime');
  fs.mkdirSync(source); fs.mkdirSync(path.join(runtimeDir, key), {recursive: true});
  fs.writeFileSync(path.join(dir, 'source.manifest.json'), JSON.stringify({tree}));
  fs.writeFileSync(path.join(directory, 'driver.rc'), '0\n'); fs.writeFileSync(path.join(runtimeDir, 'native.key'), key + '\n');
  const pack = path.join(runtimeDir, key, 'jet_runtime.pack.o'), jet = path.join(directory, 'jet');
  fs.writeFileSync(pack, 'pack digest fixture'); fs.writeFileSync(jet, 'candidate digest fixture');
  const hash = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
  const receipt = {schema: 'jet-native-runtime-proof/v1', commit: head, tree, source, runtime_dir: runtimeDir, runtimeKey: key, packSha256: hash(pack), candidate: {sha256: hash(jet)}, rustObjects: 0, llvmObjects: 0};
  const file = path.join(directory, 'receipt.json'); fs.writeFileSync(file, JSON.stringify(receipt));
  return {source, directory, receipt, file, pack};
}

test('outer pack receipt is bound to native.key and the selected pack digest', t => {
  const fixture = packFixture(temporary(t)), options = {directory: fixture.directory, source: fixture.source, head};
  assert.equal(readSharedPack(options).pack, fixture.pack);
  fs.writeFileSync(fixture.file, JSON.stringify({...fixture.receipt, runtimeKey: 'd'.repeat(64)}));
  assert.throws(() => readSharedPack(options), /runtimeKey/);
  fs.writeFileSync(fixture.file, JSON.stringify({...fixture.receipt, packSha256: 'd'.repeat(64)}));
  assert.throws(() => readSharedPack(options), /digest differs/);
});

test('malformed and failed producer receipts cannot become published runtime inputs', t => {
  const fixture = packFixture(temporary(t)), options = {directory: fixture.directory, source: fixture.source, head};
  fs.writeFileSync(fixture.file, '{'); assert.throws(() => readSharedPack(options));
  fs.writeFileSync(fixture.file, JSON.stringify(fixture.receipt));
  fs.writeFileSync(path.join(fixture.directory, 'driver.rc'), '1\n');
  assert.throws(() => readSharedPack(options), /real driver/);
});

test('all six generated S5 holds have scheduler-valid kinds and shell syntax', t => {
  const dir = temporary(t);
  const generator = fs.readFileSync(new URL('./make-jobs.sh', import.meta.url), 'utf8');
  const start = generator.indexOf('\njob() {');
  assert.ok(start >= 0, 'job generator function must exist');
  const env = {...process.env, Q: dir, C: dir, P: 'Main-RuntimeLead12-s5',
    HOST: 'Main-test-host', PACK: 'Main-test-pack', SHA: head};
  const generated = spawnSync('bash', ['-euo', 'pipefail'], {
    input: generator.slice(start), env, encoding: 'utf8',
  });
  assert.equal(generated.status, 0, generated.stderr);
  for (const phase of ['build', 'gates', 'runtime', 'boundary', 'goldens', 'fixtures']) {
    const file = path.join(dir, `Main-RuntimeLead12-s5-${phase}-r4.sh.mainhold`);
    const text = fs.readFileSync(file, 'utf8');
    const header = text.split('\n')[1];
    assert.match(header, /^# proofq: mem=\d+G time=\d+M kind=(build|test|measure) after=/);
    assert.ok(header.includes(`kind=${phase === 'build' ? 'build' : 'test'} `));
    const syntax = spawnSync('bash', ['-n', file], {encoding: 'utf8'});
    assert.equal(syntax.status, 0, syntax.stderr);
  }
});
