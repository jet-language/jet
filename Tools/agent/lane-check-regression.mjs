#!/usr/bin/env node
// Exercise the real lane shell with tiny fixture children, not a compiler build.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';

const source = fileURLToPath(new URL('./lane-check.sh', import.meta.url));
// lane-check runs the personal-info guard first; the guard reads Tower's rule table.
const guard = fileURLToPath(new URL('./personal-info-guard.sh', import.meta.url));
const rules = fileURLToPath(new URL('../tower/app/personal-info.mjs', import.meta.url));
const parent = process.argv[2] ?? os.tmpdir();
assert(path.resolve(parent) !== '/tmp' && !path.resolve(parent).startsWith('/tmp/'),
  'Use disk-backed fixture scratch');
const root = fs.mkdtempSync(path.join(parent, 'jet-lane-regression.'));
const repo = path.join(root, 'repo');
const bin = path.join(root, 'bin');
fs.mkdirSync(path.join(repo, 'Tools/agent'), {recursive: true});
fs.mkdirSync(path.join(repo, 'Tools/tower/app'), {recursive: true});
fs.mkdirSync(bin);
fs.copyFileSync(source, path.join(repo, 'Tools/agent/lane-check.sh'));
fs.copyFileSync(guard, path.join(repo, 'Tools/agent/personal-info-guard.sh'));
fs.copyFileSync(rules, path.join(repo, 'Tools/tower/app/personal-info.mjs'));
// The memory-capped scope is a pass-through here: drop systemd-run's options.
fs.writeFileSync(path.join(bin, 'systemd-run'), '#!/usr/bin/env bash\nwhile [ "$#" -gt 0 ]; do case "$1" in -p) shift 2 ;; -*) shift ;; *) break ;; esac; done\nexec "$@"\n', {mode: 0o755});
fs.writeFileSync(path.join(bin, 'jet'), `#!/usr/bin/env bash
printf '%s\\n' "$JET_KEEP_GOING" "$@" > "$LANE_ARGS_PATH"
case "$LANE_FIXTURE" in
  success) printf 'Finished fixture check\\n'; exit 0 ;;
  errors)
    for n in 1 2 3 4 5 6 7 8; do
      printf 'error[E0308]: independent error %s\\n  --> src/shared.jet:%s:1\\n' "$n" "$n" >&2
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
fs.writeFileSync(path.join(repo, 'focused.jet'), '');

const outcomes = [];
for (const [name, mode, args, expected] of [
  ['default-inventory', 'success', [], 0],
  ['focused', 'success', ['focused.jet'], 0],
  ['all-errors', 'errors', ['focused.jet'], 101],
  ['no-diagnostic', 'no-diagnostic', ['focused.jet'], 17],
  ['interrupted', 'interrupted', ['focused.jet'], 124],
]) {
  const scratch = path.join(root, name);
  fs.mkdirSync(scratch);
  const env = {...process.env, PATH: `${bin}:${process.env.PATH}`, JET_TEST_SCRATCH: scratch, XDG_CACHE_HOME: path.join(root, 'cache'),
    JET: path.join(bin, 'jet'), JET_MIN_FREE_MB: '0', JET_CHECK_SLOTS: '1', LANE_FIXTURE: mode,
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
  assert.equal(logs.length, 1, `${name}: one complete durable compiler stream`);
  const log = path.join(scratch, logs[0]);
  const text = fs.readFileSync(log, 'utf8');
  const actualArgs = fs.readFileSync(env.LANE_ARGS_PATH, 'utf8').trim().split('\n');
  assert.equal(actualArgs[0], '1', `${name}: keep-going collects every error`);
  assert.equal(actualArgs[1], 'check');
  if (args.length === 0) assert.equal(actualArgs[2], path.join(repo, 'Compiler'));
  else assert.equal(actualArgs[2], path.join(repo, 'focused.jet'));
  if (expected === 0) {
    assert.equal(result.stdout, 'CHECK OK\n');
    assert.equal(fs.readFileSync(`${log}.rc`, 'utf8'), '0\n');
  } else if (mode === 'errors') {
    assert(result.stdout.includes('8 error(s) in 1 file(s)'));
    for (let n = 1; n <= 8; n++) assert(result.stdout.includes(`src/shared.jet:${n}:1: error[E0308]: independent error ${n}`));
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
// The personal-info guard runs first and refuses a checkout naming this machine's home.
{
  const scratch = path.join(root, 'personal-info');
  fs.mkdirSync(scratch);
  const leak = path.join(repo, 'leak.md');
  fs.writeFileSync(leak, `notes kept in ${os.homedir()}/notes\n`);
  const result = spawnSync('bash', [path.join(repo, 'Tools/agent/lane-check.sh'), 'focused.jet'], {encoding: 'utf8', timeout: 10000,
    env: {...process.env, PATH: `${bin}:${process.env.PATH}`, JET_TEST_SCRATCH: scratch, XDG_CACHE_HOME: path.join(root, 'cache'),
      JET: path.join(bin, 'jet'), LANE_FIXTURE: 'success', LANE_ARGS_PATH: path.join(scratch, 'args.txt')}});
  fs.renameSync(leak, path.join(scratch, 'leak.md'));
  assert.equal(result.status, 1, `personal-info: ${result.stderr}`);
  assert(result.stdout.includes('CHECK FAILED — personal or machine information'), result.stdout);
  assert(result.stderr.includes('leak.md:1:'), result.stderr);
  assert(!fs.existsSync(path.join(scratch, 'args.txt')), 'personal-info: refused before any compiler run');
  outcomes.push({name: 'personal-info', actualRc: result.status, regression: 'PASS'});
}
console.log(JSON.stringify({scope: 'Real lane shell with fixture children only', outcomes, green: true}, null, 2));
