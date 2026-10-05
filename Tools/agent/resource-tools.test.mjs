import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

const tools = path.dirname(fileURLToPath(import.meta.url));
const repo = fs.realpathSync(path.join(tools, '../..'));
const state = process.env.JET_DEV_ROOT || path.join(process.env.XDG_CACHE_HOME || path.join(process.env.HOME, '.cache'), 'jet-dev');
fs.mkdirSync(path.join(state, 'scratch'), { recursive: true });

function fixture(t) {
  const root = fs.mkdtempSync(path.join(state, 'scratch', 'resource-tools-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  for (const name of ['bin', 'scratch', 'proofq', 'outputs']) fs.mkdirSync(path.join(root, name));
  fs.writeFileSync(path.join(root, 'bin/direnv'), '#!/usr/bin/env bash\nexit 0\n', { mode: 0o755 });
  fs.writeFileSync(path.join(root, 'bin/df'), '#!/usr/bin/env bash\nprintf "size used avail pcent type\\n1000000000000 %s %s %s%% %s\\n" "${TEST_USED:-100000000000}" "${TEST_AVAIL:-900000000000}" "${TEST_PERCENT:-10}" "${TEST_FSTYPE:-ext4}"\n', { mode: 0o755 });
  const env = {
    ...process.env,
    PATH: `${path.join(root, 'bin')}:${process.env.PATH}`,
    JET_HOST_ENV_FILE: path.join(root, 'host-env'),
    JET_SCRATCH_ROOT: path.join(root, 'scratch'),
    JET_DEV_ROOT: root,
    JET_PROOFQ_ROOT: path.join(root, 'proofq'),
    JET_REPO: repo,
    JET_DISK_ROOT: root,
    CARGO_TARGET_DIR: path.join(root, 'scratch/targets/worker'),
    TEST_USED: '100000000000', TEST_AVAIL: '900000000000', TEST_PERCENT: '10', TEST_FSTYPE: 'ext4',
  };
  const run = (script, args = [], extra = {}) => spawnSync('bash', [path.join(tools, script), ...args], {
    cwd: repo, env: { ...env, ...extra }, encoding: 'utf8', timeout: 10000,
  });
  return { root, env, run };
}

test('host defaults propagate without a silent scratch fallback', t => {
  const f = fixture(t);
  fs.writeFileSync(f.env.JET_HOST_ENV_FILE, `JET_SCRATCH_ROOT='${f.env.JET_SCRATCH_ROOT}'\n`);
  const result = f.run('disk-guard.sh', ['--check', '1', path.join(f.root, 'outputs')], { JET_SCRATCH_ROOT: '' });
  assert.equal(result.status, 0, result.stderr);
  fs.unlinkSync(f.env.JET_HOST_ENV_FILE);
  const missing = f.run('disk-guard.sh', ['--check', '1', f.root], { JET_SCRATCH_ROOT: '' });
  assert.notEqual(missing.status, 0);
  assert.match(missing.stderr, /configure JET_SCRATCH_ROOT/);
});

test('jet-env stamps an external target and rejects a different checkout owner', t => {
  const f = fixture(t);
  const result = f.run('jet-env', ['bash', '-c', 'printf resources-ok']);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout, 'resources-ok');
  const owner = path.join(f.env.CARGO_TARGET_DIR, '.jet-checkout');
  assert.equal(fs.readFileSync(owner, 'utf8').trim(), repo);
  fs.writeFileSync(owner, `${path.join(f.root, 'another-checkout')}\n`);
  const mismatch = f.run('jet-env', ['bash', '-c', 'printf must-not-run']);
  assert.equal(mismatch.status, 64);
  assert.equal(mismatch.stdout, '');
  assert.match(mismatch.stderr, /belongs to/);
});

test('jet-env rejects unconfigured external and RAM-backed targets', t => {
  const f = fixture(t);
  assert.equal(f.run('jet-env', ['true'], { JET_SCRATCH_ROOT: '' }).status, 64);
  assert.equal(f.run('jet-env', ['true'], { CARGO_TARGET_DIR: '/tmp/jet-resource-test' }).status, 64);
  assert.equal(f.run('jet-env', ['true'], { CARGO_TARGET_DIR: path.join(f.root, 'outputs') }).status, 64);
});

test('disk peak gate checks headroom, pause, mount, and filesystem type', t => {
  const f = fixture(t);
  const args = ['--check', '30', path.join(f.root, 'outputs')];
  assert.equal(f.run('disk-guard.sh', args).status, 0);
  assert.equal(f.run('disk-guard.sh', args, { TEST_USED: '840000000000', TEST_AVAIL: '160000000000', TEST_PERCENT: '84' }).status, 75);
  assert.equal(f.run('disk-guard.sh', args, { TEST_AVAIL: '1' }).status, 75);
  assert.equal(f.run('disk-guard.sh', args, { TEST_FSTYPE: 'tmpfs' }).status, 75);
  // A scratch directory on the main device is not a mounted scratch filesystem.
  assert.equal(f.run('disk-guard.sh', ['--check', '1', f.env.JET_SCRATCH_ROOT]).status, 75);
  fs.writeFileSync(path.join(f.env.JET_PROOFQ_ROOT, 'PAUSE'), 'owner pause\n');
  assert.equal(f.run('disk-guard.sh', args).status, 75);
});

test('guard pauses above 85%, recovers below 80%, and never releases an owner pause', t => {
  const f = fixture(t);
  const pause = path.join(f.env.JET_PROOFQ_ROOT, 'PAUSE');
  const high = f.run('disk-guard.sh', ['--once'], { TEST_PERCENT: '85' });
  assert.equal(high.status, 0, high.stderr);
  assert.equal(fs.readFileSync(pause, 'utf8'), 'disk-guard recovery\n');
  assert.equal(f.run('disk-guard.sh', ['--once']).status, 0);
  assert.equal(fs.existsSync(pause), false);
  fs.writeFileSync(pause, 'owner pause\n');
  assert.equal(f.run('disk-guard.sh', ['--once']).status, 0);
  assert.equal(fs.readFileSync(pause, 'utf8'), 'owner pause\n');
});

test('protected evidence stays in place and missing protection fails closed', t => {
  const f = fixture(t);
  const target = path.join(f.root, 'outputs/target');
  fs.mkdirSync(target);
  fs.writeFileSync(path.join(target, 'evidence.json'), '{}\n');
  fs.writeFileSync(path.join(f.root, 'DISK-PLAN.tsv'), `path\tsize_bytes\tclass\tsafe\treason\tlast_modified\n${target}\t3\tscratch-target\tyes\tidle\t2000-01-01T00:00:00Z\n`);
  fs.writeFileSync(path.join(f.root, 'disk-protect.txt'), `${target}/*\n`);
  const protectedRun = f.run('disk-guard.sh', ['--once'], { TEST_PERCENT: '85' });
  assert.equal(protectedRun.status, 0, protectedRun.stderr);
  assert.equal(fs.existsSync(path.join(target, 'evidence.json')), true);
  // An exact child protects its parent too.
  fs.writeFileSync(path.join(f.root, 'disk-protect.txt'), `${target}/evidence.json\n`);
  assert.equal(f.run('disk-guard.sh', ['--once'], { TEST_PERCENT: '85' }).status, 0);
  assert.equal(fs.existsSync(path.join(target, 'evidence.json')), true);
  fs.unlinkSync(path.join(f.root, 'disk-protect.txt'));
  const missing = f.run('disk-guard.sh', ['--once'], { TEST_PERCENT: '85' });
  assert.equal(missing.status, 1);
  assert.match(fs.readFileSync(path.join(f.root, 'DISK-ALERT'), 'utf8'), /failed closed/);
  assert.equal(fs.existsSync(path.join(target, 'evidence.json')), true);
});

test('both queue lanes respect PAUSE and the 88% disk gate before claiming', t => {
  const f = fixture(t);
  const queue = path.join(f.env.JET_PROOFQ_ROOT, 'queue');
  fs.mkdirSync(queue);
  const job = path.join(queue, 'Fix-resource-test.sh');
  fs.writeFileSync(job, '#!/usr/bin/env bash\nexit 0\n', { mode: 0o755 });
  const claim = (lane, extra = {}) => spawnSync(process.execPath, [path.join(tools, 'proof-queue.mjs'), '--claim', String(lane)], {
    env: { ...f.env, ...extra }, encoding: 'utf8', timeout: 10000,
  });
  fs.writeFileSync(path.join(f.env.JET_PROOFQ_ROOT, 'PAUSE'), 'owner pause\n');
  for (const lane of [1, 2]) {
    const result = claim(lane);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout, '');
    assert.equal(fs.existsSync(job), true);
  }
  fs.unlinkSync(path.join(f.env.JET_PROOFQ_ROOT, 'PAUSE'));
  for (const lane of [1, 2]) {
    const result = claim(lane, { TEST_PERCENT: '88' });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout, '');
    assert.equal(fs.existsSync(job), true);
  }
});
