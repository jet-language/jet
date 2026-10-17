#!/usr/bin/env node
// Exercise the real lane shell with tiny fixture children, not a Cargo build.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';

const source = fileURLToPath(new URL('./lane-check.sh', import.meta.url));
const parent = process.argv[2] ?? os.tmpdir();
assert(path.resolve(parent) !== '/tmp' && !path.resolve(parent).startsWith('/tmp/'),
  'Use disk-backed fixture scratch');
const root = fs.mkdtempSync(path.join(parent, 'jet-lane-regression.'));
const repo = path.join(root, 'repo');
const bin = path.join(root, 'bin');
fs.mkdirSync(path.join(repo, 'Tools/agent'), {recursive: true});
fs.mkdirSync(bin);
fs.copyFileSync(source, path.join(repo, 'Tools/agent/lane-check.sh'));
fs.writeFileSync(path.join(repo, 'Tools/agent/jet-env'), '#!/usr/bin/env bash\nexec "$@"\n', {mode: 0o755});
fs.writeFileSync(path.join(bin, 'cargo'), `#!/usr/bin/env bash
printf '%s\\n' "$@" > "$LANE_ARGS_PATH"
case "$LANE_FIXTURE" in
  success) printf 'Finished fixture check\\n'; exit 0 ;;
  errors)
    for n in 1 2 3 4 5 6 7 8; do
      printf 'error[E0308]: independent error %s\\n  --> src/shared.rs:%s:1\\n' "$n" "$n" >&2
    done
    printf '%2200s\\ncomplete diagnostic tail retained\\n' '' >&2
    exit 101 ;;
  no-diagnostic)
    printf 'beginning of complete non-diagnostic failure\\n%2200s\\nend of complete non-diagnostic failure\\n' '' >&2
    exit 17 ;;
  interrupted)
    printf 'live compiler output before interruption\\n' >&2
    sleep 30 ;;
  *) exit 64 ;;
esac
`, {mode: 0o755});

const outcomes = [];
for (const [name, mode, args, expected] of [
  ['default-inventory', 'success', [], 0],
  ['focused', 'success', ['-p', 'fixture'], 0],
  ['explicit-keep-going', 'success', ['--keep-going', '-p', 'fixture'], 0],
  ['all-errors', 'errors', ['-p', 'fixture'], 101],
  ['no-diagnostic', 'no-diagnostic', ['-p', 'fixture'], 17],
  ['interrupted', 'interrupted', ['-p', 'fixture'], 124],
]) {
  const scratch = path.join(root, name);
  fs.mkdirSync(scratch);
  const env = {...process.env, PATH: `${bin}:${process.env.PATH}`, JET_TEST_SCRATCH: scratch,
    JET_MIN_FREE_MB: '0', JET_CHECK_SLOTS: '1', LANE_FIXTURE: mode,
    LANE_ARGS_PATH: path.join(scratch, 'args.txt')};
  const command = name === 'interrupted' ? 'timeout' : 'bash';
  const commandArgs = name === 'interrupted'
    ? ['--kill-after=1s', '1s', 'bash', path.join(repo, 'Tools/agent/lane-check.sh'), ...args]
    : [path.join(repo, 'Tools/agent/lane-check.sh'), ...args];
  const result = spawnSync(command, commandArgs, {env, encoding: 'utf8', timeout: 10000});
  fs.writeFileSync(path.join(scratch, 'stdout.log'), result.stdout ?? '');
  fs.writeFileSync(path.join(scratch, 'stderr.log'), result.stderr ?? '');
  assert.equal(result.error, undefined, `${name}: ${result.error}`);
  if (mode === 'interrupted') {
    assert(result.status === 124 || result.status === 137
      || (result.status === null && result.signal === 'SIGKILL'),
    `${name}: preserve the actual timeout/kill result`);
  } else {
    assert.equal(result.status, expected, `${name}: ${result.stderr}`);
  }
  const logs = fs.readdirSync(scratch).filter(file => /^jet-lane-check\.[^.]+$/.test(file));
  assert.equal(logs.length, 1, `${name}: one complete durable Cargo stream`);
  const log = path.join(scratch, logs[0]);
  const text = fs.readFileSync(log, 'utf8');
  const actualArgs = fs.readFileSync(env.LANE_ARGS_PATH, 'utf8').trim().split('\n');
  assert.equal(actualArgs[0], 'check');
  assert.equal(actualArgs.filter(arg => arg === '--keep-going').length, 1);
  if (args.length === 0) assert.deepEqual(actualArgs.slice(1),
    ['--keep-going', '--workspace', '--all-targets', '--features', 'jet/compiler-bootstrap-host']);
  if (expected === 0) {
    assert.equal(result.stdout, 'CHECK OK\n');
    assert.equal(fs.readFileSync(`${log}.rc`, 'utf8'), '0\n');
  } else if (mode === 'errors') {
    assert(result.stdout.includes('8 error(s) in 1 file(s)'));
    for (let n = 1; n <= 8; n++) assert(result.stdout.includes(`src/shared.rs:${n}:1: error[E0308]: independent error ${n}`));
    assert(text.includes('complete diagnostic tail retained'));
    assert(result.stdout.includes(text), 'entire compiler output, not the first error excerpt');
    assert.equal(fs.readFileSync(`${log}.rc`, 'utf8'), '101\n');
  } else if (mode === 'no-diagnostic') {
    assert(result.stdout.includes(text), 'retain the beginning and end without truncation');
    assert.equal(fs.readFileSync(`${log}.rc`, 'utf8'), '17\n');
  } else {
    assert(text.includes('live compiler output before interruption'));
    assert(!result.stdout.includes('CHECK OK'));
    if (fs.existsSync(`${log}.rc`)) {
      assert.notEqual(fs.readFileSync(`${log}.rc`, 'utf8'), '0\n');
    } else {
      assert(result.signal === 'SIGKILL' || result.status === 137,
        'only a forced kill may leave the terminal status unknown');
    }
  }
  outcomes.push({name, actualRc: result.status, actualSignal: result.signal,
    log, terminalRcRecorded: fs.existsSync(`${log}.rc`),
    bytes: Buffer.byteLength(text), regression: 'PASS'});
}
console.log(JSON.stringify({scope: 'Real lane shell with fixture children only', outcomes, green: true}, null, 2));
