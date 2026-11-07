// clean-nix-tmp.sh regression (#6131): the cleaner one dev shell runs must
// keep every live shell's TMPDIR and still remove exited shells' dirs.
// Shells are emulated the way `nix develop` builds them: the rcfile creates
// nix-shell.XXXXXX and exports TMPDIR after exec, then runs the flake
// shellHook's TMPDIR hold (read verbatim from flake.nix) and, for the second
// shell, the cleaner. JET_NIX_TMP_ROOT keeps the fixture dirs out of /tmp.
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const cleanerPath = join(repo, 'Tools/agent/clean-nix-tmp.sh');

function hookHold() {
  const holds = readFileSync(join(repo, 'flake.nix'), 'utf8').split('\n')
    .filter((line) => line.includes('exec {jet_nix_tmp_fd}<'))
    .map((line) => line.trim().replaceAll("''${", '${'));
  assert.equal(holds.length, 2, 'both dev shellHooks hold their TMPDIR open');
  assert.equal(holds[0], holds[1]);
  return holds[0];
}

function rcfile(hold, tail) {
  return [
    'export NIX_BUILD_TOP="$(mktemp -d "$JET_NIX_TMP_ROOT/nix-shell.XXXXXX")"',
    'export TMP="$NIX_BUILD_TOP" TMPDIR="$NIX_BUILD_TOP" TEMP="$NIX_BUILD_TOP" TEMPDIR="$NIX_BUILD_TOP"',
    ...(hold ? [hold] : []),
    tail,
  ].join('\n');
}

const reportAndIdle = 'printf "%s\\n" "$TMPDIR"; read -r _';
const runCleaner = '"$JET_ROOT/Tools/agent/clean-nix-tmp.sh"; printf "%s\\n" "$TMPDIR"';

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'jet-nix-tmp-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  assert.match(root, /^\/[A-Za-z0-9/._-]+$/, 'fixture root must be a valid JET_NIX_TMP_ROOT');
  return { root, env: { ...process.env, JET_NIX_TMP_ROOT: root, JET_ROOT: repo } };
}

// A shell left at its prompt: `read` is a builtin, so no child process holds
// or names the dir; only what the shell itself holds can keep it live.
async function idleShell(t, env, script) {
  const child = spawn('bash', ['-c', script], { cwd: '/', env, stdio: ['pipe', 'pipe', 'inherit'] });
  t.after(() => {
    if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL');
  });
  const dir = await new Promise((resolveDir, reject) => {
    let out = '';
    child.stdout.setEncoding('utf8');
    child.stdout.on('data', (chunk) => {
      out += chunk;
      if (out.includes('\n')) resolveDir(out.slice(0, out.indexOf('\n')));
    });
    child.on('error', reject);
    child.on('exit', (code) => reject(new Error(`shell exited (${code}) before reporting TMPDIR`)));
  });
  assert.ok(existsSync(dir), `${dir} exists while its shell is idle`);
  return dir;
}

function run(argv, env) {
  const result = spawnSync(argv[0], argv.slice(1), { cwd: '/', env, encoding: 'utf8', timeout: 120_000 });
  assert.equal(result.status, 0, `${argv.join(' ')} failed:\n${result.stderr}`);
  return result.stdout.trim();
}

test('a live shell keeps its TMPDIR through a second shell\'s cleaner; an exited shell\'s dir is removed', async (t) => {
  const { env } = fixture(t);
  const hold = hookHold();
  const live = await idleShell(t, env, rcfile(hold, reportAndIdle));
  const exited = run(['bash', '-c', rcfile(hold, 'printf "%s\\n" "$TMPDIR"')], env);
  assert.ok(existsSync(exited));
  const second = run(['bash', '-c', rcfile(hold, runCleaner)], env);
  assert.ok(existsSync(live), 'the idle shell\'s TMPDIR survives');
  assert.ok(existsSync(second), 'the cleaning shell keeps its own TMPDIR');
  assert.ok(!existsSync(exited), 'the exited shell\'s TMPDIR is removed');
});

test('control: without the shellHook hold the same idle shell looks dead', async (t) => {
  const { env } = fixture(t);
  const live = await idleShell(t, env, rcfile(null, reportAndIdle));
  run(['bash', '-c', rcfile(hookHold(), runCleaner)], env);
  assert.ok(!existsSync(live), 'an unheld TMPDIR exported after exec is invisible to the scan');
});

test('a cleaner inside a user namespace removes nothing', async (t) => {
  if (spawnSync('unshare', ['-r', 'true']).status !== 0) {
    t.skip('unprivileged user namespaces are unavailable');
    return;
  }
  const { root, env } = fixture(t);
  // `nix develop -c CMD` shape: the command's own environ names TMPDIR.
  const named = mkdtempSync(join(root, 'nix-shell.'));
  await idleShell(t, { ...env, TMPDIR: named }, reportAndIdle);
  const exited = mkdtempSync(join(root, 'nix-shell.'));
  run(['unshare', '-r', 'bash', '-c', rcfile(hookHold(), runCleaner)], env);
  assert.ok(existsSync(named), 'a live shell outside the namespace survives');
  assert.ok(existsSync(exited), 'nothing is removed when the scan cannot see the parent namespace');
  run(['bash', '-c', rcfile(hookHold(), runCleaner)], env);
  assert.ok(existsSync(named));
  assert.ok(!existsSync(exited), 'an unconfined cleaner removes the dead dir');
});

test('liveness stays a batch scan', () => {
  const code = readFileSync(cleanerPath, 'utf8').split('\n').filter((line) => !line.trim().startsWith('#')).join('\n');
  assert.doesNotMatch(code, /\bfor\s+\w+\s+in\s+[^;\n]*\/proc/, 'no per-process loop over /proc');
  assert.doesNotMatch(code, /\breadlink\b/, 'no per-link readlink forks');
});
