import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { startupTiming, startupReceipt, startupInputHash, compareStartup, compareStartupDirectory } from './startup.mjs';

const events = (parse, end) => [
  { name: 'compile', ph: 'B', ts: 1 },
  { name: 'main', cat: 'parse', ph: 'B', ts: parse },
  { name: 'compile', ph: 'E', ts: end },
];
const timing = wall => `Elapsed (wall clock) time (h:mm:ss or m:ss): ${wall}\n`;
const receipt = identity => ({ complete: true, report_count: 0, source_bytes: 123, selected_factory_tier: 'Aot', actual_factory_tier: 'Aot', compiler_identity: identity });
const receiptText = identity => Object.entries(receipt(identity)).map(([key, value]) => `${key}=${value}`).join('\n') + '\n';
const row = ns => ({ receipt: receipt('compiler'), source_sha256: 'c'.repeat(64), artifact_sha256: 'a'.repeat(64), timing: { wall_ns: ns, compile_ns: ns, pre_parse_ns: ns } });

test('startup timings use compile and first parse trace boundaries plus process wall time', () => {
  assert.deepEqual(startupTiming(events(101, 201), timing('0:03.33')), { wall_ns: 3330000000n, compile_ns: 200000n, pre_parse_ns: 100000n });
  assert.equal(startupTiming(events(101, 201), timing('1:02:03.33')).wall_ns, 3723330000000n);
  assert.throws(() => startupTiming(events(1, 201), timing('0:03.33')), /boundaries/);
  assert.throws(() => startupTiming(events(101, 101), timing('0:03.33')), /boundaries/);
  assert.throws(() => startupTiming(events(101, 201), ''), /wall time/);
});

test('startup receipt parses the actual line-oriented canonical Aot receipt', () => {
  assert.deepEqual(startupReceipt(receiptText('compiler')), receipt('compiler'));
  assert.throws(() => startupReceipt('not a receipt'), /canonical startup receipt/);
});

test('startup pair fails closed on equality, incomplete compile, output drift and identity changes', () => {
  const base = Array.from({ length: 3 }, () => row(10n));
  const fast = Array.from({ length: 3 }, () => row(9n));
  assert.equal(compareStartup(base, fast).verdict, 'win');
  fast[0].timing.pre_parse_ns = 10n;
  fast[1].timing.pre_parse_ns = 10n;
  assert.equal(compareStartup(base, fast).verdict, 'loss');
  assert.throws(() => compareStartup(base.slice(1), fast.slice(1)), /three matched/);
  fast[0].receipt.report_count = 1;
  assert.throws(() => compareStartup(base, fast), /did not complete/);
  fast[0].receipt.report_count = 0;
  fast[0].artifact_sha256 = 'b'.repeat(64);
  assert.throws(() => compareStartup(base, fast), /emitted artifact/);
  fast[0].artifact_sha256 = 'a'.repeat(64);
  fast[0].receipt.compiler_identity = 'changed';
  assert.throws(() => compareStartup(base, fast), /identity changed/);
  fast[0].receipt.compiler_identity = 'compiler';
  fast[0].source_sha256 = 'd'.repeat(64);
  assert.throws(() => compareStartup(base, fast), /actual project inputs/);
  const unavailable = Array.from({ length: 3 }, () => ({ ...row(9n), receipt: receipt('unavailable') }));
  assert.throws(() => compareStartup(unavailable, unavailable), /identity unavailable/);
});

test('startup filesystem cell reads matching immutable trial evidence and rejects process failure', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'jet-startup-test-'));
  try {
    const inputRoot = path.join(root, 'inputs');
    fs.mkdirSync(inputRoot);
    fs.writeFileSync(path.join(inputRoot, 'main.jet'), 'fn run() {}');
    fs.writeFileSync(path.join(inputRoot, 'package.jet'), 'name: "startup_fixture"');
    const source_sha256 = startupInputHash(inputRoot);
    fs.writeFileSync(path.join(root, 'pair.json'), JSON.stringify({ schema: 'jet.native-host-shape-pair.v1', source_sha256, machine: { cpu: 'fixture' }, build_profile: 'same-profile' }));
    for (const arm of ['plain', 'optimized']) {
      for (let trial = 1; trial <= 3; trial++) {
        const dir = path.join(root, arm, `trial-00${trial}`);
        fs.mkdirSync(dir, { recursive: true });
        fs.writeFileSync(path.join(dir, 'rc'), '0\n');
        fs.writeFileSync(path.join(dir, 'receipt'), receiptText(arm));
        fs.writeFileSync(path.join(dir, 'invocation.json'), JSON.stringify({ source_root: inputRoot, source_sha256 }));
        fs.writeFileSync(path.join(dir, 'out.rs'), 'identical emitted compiler artifact');
        fs.writeFileSync(path.join(dir, 'trace.json'), JSON.stringify(events(arm === 'plain' ? 101 : 51, arm === 'plain' ? 201 : 101)));
        fs.writeFileSync(path.join(dir, 'time.txt'), timing(arm === 'plain' ? '0:03.33' : '0:02.00'));
      }
    }
    const result = compareStartupDirectory(root);
    assert.equal(result.verdict, 'win');
    assert.deepEqual(result.trial_directories, ['trial-001', 'trial-002', 'trial-003']);
    fs.writeFileSync(path.join(root, 'optimized', 'trial-001', 'rc'), '101\n');
    assert.throws(() => compareStartupDirectory(root), /process failed/);
    fs.writeFileSync(path.join(root, 'optimized', 'trial-001', 'rc'), '0\n');
    fs.renameSync(path.join(root, 'optimized', 'trial-003'), path.join(root, 'optimized', 'unmatched'));
    assert.throws(() => compareStartupDirectory(root), /unmatched startup/);
    fs.renameSync(path.join(root, 'optimized', 'unmatched'), path.join(root, 'optimized', 'trial-003'));
    const invocation = path.join(root, 'optimized', 'trial-003', 'invocation.json');
    fs.writeFileSync(invocation, JSON.stringify({ source_root: inputRoot, source_sha256: 'd'.repeat(64) }));
    assert.throws(() => compareStartupDirectory(root), /actual matched input snapshot/);
    fs.writeFileSync(invocation, JSON.stringify({ source_root: inputRoot, source_sha256 }));
    fs.writeFileSync(path.join(inputRoot, 'main.jet'), 'fn run() { print("different"); }');
    assert.throws(() => compareStartupDirectory(root), /input snapshot differs/);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});
