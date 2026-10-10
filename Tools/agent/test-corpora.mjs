#!/usr/bin/env node
// Cargo-free stream/snapshot gate. Corpus discovery and interactive inputs reuse
// compiler-diff; this is an expectation comparator, not a two-compiler diff.
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, relative, resolve, sep } from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { collectGoldenEntries, goldenFailureExit, loadExampleStdin } from './compiler-diff.mjs';
import { copyFeatureProject, featureProjectRoot } from './run-feature-examples.mjs';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const EXPECTED = join(ROOT, 'Examples/features/expected');
const optional = (path) => existsSync(path) ? readFileSync(path, 'utf8') : null;
const verdict = (status, detail = '') => ({ status, detail });

// No newline trimming, CRLF folding, or warning-as-success rule: golden.rs
// compares exact UTF-8 streams and only accepts 1/70 for an expected failure.
export function goldenVerdict(entry, run, expected, binary) {
  if (expected.err !== null) {
    const want = goldenFailureExit(entry.stem, expected.err);
    if (want === null) return verdict('FAIL', 'expected failure exit is ambiguous');
    if (run.code !== want) return verdict('FAIL', `exit ${run.code}; expected ${want}`);
    if (run.stdout !== (expected.out ?? '')) return verdict('FAIL', 'stdout mismatch');
    return run.stderr === expected.err ? verdict('PASS') : verdict('FAIL', 'stderr mismatch');
  }
  if (run.code !== 0) return verdict('FAIL', `exit ${run.code}`);
  if (expected.out === null) return verdict('REFUSED', 'missing stdout golden');
  const stdout = entry.stem.startsWith('cli/')
    ? run.stdout.replace(`Usage: ${basename(binary)}`, `Usage: ${basename(entry.path, '.jet')}`)
    : run.stdout;
  if (entry.stem === 'tooling/compute_vulkan_webgpu') {
    const lines = stdout.split('\n');
    if (lines.at(-1) === '') lines.pop();
    if (lines.length !== 2 || !['vulkan:accepted', 'vulkan:rejected'].includes(lines[0]) || lines[1] !== 'webgpu:rejected') return verdict('FAIL', 'compute availability contract mismatch');
  } else if (stdout !== expected.out) return verdict('FAIL', 'stdout mismatch');
  if (expected.stderrOut !== null && run.stderr !== expected.stderrOut) return verdict('FAIL', 'stderr mismatch');
  return verdict('PASS');
}

export function uiEntries(root = ROOT, suite = 'ui') {
  const directory = join(root, 'tests', suite === 'lint' ? 'ui_lint' : 'ui'), result = [];
  for (const item of readdirSync(directory, { withFileTypes: true })) {
    const path = join(directory, item.name);
    if (item.isFile() && item.name.endsWith('.jet') && (suite === 'lint' || !item.name.includes('.fixed.'))) {
      result.push({ path, shown: relative(root, path), stem: relative(root, path), expected: path.slice(0, -4) + (suite === 'lint' ? '.warn' : '.stderr'), suite });
    } else if (suite === 'ui' && item.isDirectory()) {
      const entry = ['run.jet', 'main.jet', 'workspace.jet'].map((name) => join(path, name)).find(existsSync);
      if (entry) result.push({ path: entry, shown: relative(root, entry), stem: relative(root, entry), expected: basename(entry) === 'workspace.jet' ? `${path}.stderr` : join(path, 'stderr'), suite });
    }
  }
  return result.sort((a, b) => a.shown.localeCompare(b.shown, 'en'));
}

export function uiInvocation(source, file, suite = 'ui') {
  const directives = new Map([...source.matchAll(/^\s*\/\/\s*@([\w-]+)(?:[ \t]+([^\n]*))?$/gmu)].map((m) => [m[1], m[2]?.trim() ?? '']));
  const has = (name) => directives.has(name), arg = (name) => directives.get(name);
  // These fixtures manufacture typed internal facts, or require a compile mode
  // not exposed by the public CLI. Never silently substitute `jet check`.
  const internal = ['lint_policy_config', 'env_facet_missing', 'workspace_lock_e1202', 'jetlib_version_mismatch', 'jetlib_effect_refused', 'library_invalid_binding', 'command_override_conflict', 'parent_control_cancel', 'ambiguous_call', 'generated_source', 'no-os', 'gate', 'build_grants', 'build_locked', 'sandbox_missing_wasm_tools', 'jetpack_hangar_digest_mismatch', 'jetpack_e1250', 'jetpack_retired_environment_flag', 'jetpack_retired_store_verbs', 'programmable_build', 'plugin_target', 'web_target', 'target', 'cost_cli'];
  const unavailable = internal.filter(has);
  if (unavailable.length) return { refused: `missing CLI fixture API: ${unavailable.join(', ')}` };
  let args = ['check', file], cwdParent = false, command = 'jet', stdin = '';
  if (has('check_cli')) { args = ['check', basename(file)]; cwdParent = true; }
  else if (has('all_diags') || basename(file) === 'workspace.jet') args = ['check', file];
  else if (has('typed_settings_cli')) args = ['check', file, '--set', 'cli_only=true'];
  else if (has('cli_e0043')) args = ['install'];
  else if (has('cli_e2101')) args = ['bench'];
  else if (has('cli_e1219')) args = ['build', file, '--profile=turbo'];
  else if (has('retired_gate_flag')) args = ['run', file, '--allow-impure'];
  else if (has('cli_build')) args = ['build', file];
  else if (has('sandbox_effect')) args = ['build', file, '--target=sandbox'];
  else if (has('web_run') || has('run_cli')) args = ['run', file];
  else if (has('dev_interpreter')) args = ['run', '--interpret', file];
  else if (has('complexity_cli')) args = ['lint', '--complexity', file, `--max=${arg('complexity_cli').split(/\s+/u)[1]}`];
  else if (has('repl_deny')) { args = ['repl', '--deny=FS']; stdin = source.split('\n').filter((line) => line.trim() && !line.trimStart().startsWith('//')).concat(':quit').join('\n'); }
  const supported = new Set(['check_cli', 'all_diags', 'typed_settings_cli', 'cli_e0043', 'cli_e2101', 'cli_e1219', 'retired_gate_flag', 'cli_build', 'sandbox_effect', 'web_run', 'run_cli', 'dev_interpreter', 'complexity_cli', 'repl_deny', 'compiler_extension']);
  const unknown = [...directives.keys()].filter((name) => !supported.has(name));
  if (unknown.length) return { refused: `unported fixture directives: ${unknown.join(', ')}` };
  const direct = ['check_cli', 'typed_settings_cli', 'cli_e0043', 'cli_e2101', 'cli_e1219', 'retired_gate_flag', 'cli_build', 'sandbox_effect', 'web_run', 'run_cli', 'dev_interpreter', 'complexity_cli', 'repl_deny'].some(has);
  return { args, cwdParent, command, stdin, extension: arg('compiler_extension'), suite, direct, allDiags: has('all_diags') || basename(file) === 'workspace.jet' };
}

export function normalizeUi(shown, actual, root = ROOT) {
  let text = actual.replaceAll(`${root}/`, '');
  if (shown.endsWith('/workspace.jet')) text = text.replaceAll(shown.slice(0, -'/workspace.jet'.length), '/tmp');
  // Exactly the Rust normalizer's line-level package-lock chatter removal.
  return text.split(/\r?\n/u).filter((line, i, all) => !(i === all.length - 1 && line === '') && !line.includes('Blocking waiting for file lock on package cache')).map((line) => `${line}\n`).join('');
}

// Public report_problems adds a summary and an explain hint after the shared
// renderer. Internal-API snapshots assert only that renderer; real CLI
// snapshots assert the full envelope. Never remove a diagnostic body.
export function renderedOnly(text) {
  return text.replace(/\n\n(?:\d+ problems? found|ok: `[^\n]+` has no errors \(\d+ warnings?\))\nrun `jet explain [^\n]+` to learn more\n$/u, '\n');
}

// Mirror of the canonical kit_ui_primary_source_headers in Tools/testkit/Ui.jet.
const primarySourceHeaders = (shown, text) => text.split('\n').map(line => line.startsWith(`  --> ${basename(shown)}:`) ? `  --> ${shown}:${line.slice(`  --> ${basename(shown)}:`.length)}` : line).join('\n');

export function snapshotVerdict(entry, run, actual, expected) {
  if (expected === null) return verdict('FAIL', 'missing snapshot');
  if (entry.suite === 'lint' && (run.code !== 0 || !/^Warning \[/mu.test(actual))) return verdict('FAIL', 'lint fixture must compile and emit a lint');
  return (entry.shown ? primarySourceHeaders(entry.shown, actual) === primarySourceHeaders(entry.shown, expected) : actual === expected) ? verdict('PASS') : verdict('FAIL', 'snapshot mismatch');
}

function invoke(binary, args, cwd, env, timeout, stdin = '') {
  return new Promise((done) => {
    const child = spawn(binary, args, { cwd, env, detached: true, stdio: ['pipe', 'pipe', 'pipe'] });
    let stdout = '', stderr = '', timedOut = false, error = null;
    child.stdout.setEncoding('utf8'); child.stderr.setEncoding('utf8');
    child.stdout.on('data', (data) => { stdout += data; }); child.stderr.on('data', (data) => { stderr += data; });
    child.on('error', (failure) => { error = failure.message; });
    child.stdin.on('error', () => {}); child.stdin.end(stdin);
    const timer = setTimeout(() => { timedOut = true; try { process.kill(-child.pid, 'SIGKILL'); } catch {} }, timeout * 1000);
    child.on('close', (code, signal) => { clearTimeout(timer); done({ code, signal, stdout, stderr, timedOut, error }); });
  });
}
const processFailure = (run) => run.error ? verdict('ERROR', run.error) : run.timedOut ? verdict('TIMEOUT') : run.signal ? verdict('ERROR', `signal ${run.signal}`) : run.code === 101 ? verdict('FAIL', 'internal compiler error') : null;

function stage(entry, scratch) {
  const cwd = join(scratch, 'source'); mkdirSync(cwd, { recursive: true });
  if (entry.suite === 'golden') {
    const project = featureProjectRoot(entry) ?? dirname(entry.path);
    copyFeatureProject(project, join(cwd, relative(ROOT, project)));
  } else if (dirname(entry.path) !== join(ROOT, 'tests/ui') && dirname(entry.path) !== join(ROOT, 'tests/ui_lint')) {
    const destination = join(cwd, relative(ROOT, dirname(entry.path)));
    copyFeatureProject(dirname(entry.path), destination);
    const state = join(dirname(entry.path), 'state');
    if (existsSync(state)) {
      rmSync(join(destination, 'state'), { recursive: true, force: true });
      copyFeatureProject(state, join(destination, '.jet'));
      if (!existsSync(join(destination, 'package.jet')) && basename(entry.path) !== 'workspace.jet' && !/^\s*package\s*\{/mu.test(readFileSync(entry.path, 'utf8'))) writeFileSync(join(destination, 'package.jet'), `name: "${basename(entry.path, '.jet')}"\nversion: "0.1.0"\n`);
    }
  } else {
    mkdirSync(join(cwd, dirname(entry.shown)), { recursive: true });
    const stem = basename(entry.path, '.jet');
    for (const sibling of readdirSync(dirname(entry.path))) if (sibling === basename(entry.path) || sibling.startsWith(`${stem}.`)) copyFileSync(join(dirname(entry.path), sibling), join(cwd, dirname(entry.shown), sibling));
  }
  return cwd;
}

async function runEntry(entry, options, scratch, baseEnv) {
  const cwd = stage(entry, scratch), env = { ...baseEnv, JET_STORE_DIR: join(options.scratch, 'store'), JET_FFI_CACHE_DIR: join(options.scratch, 'ffi'), JETPACK_ROOT: join(scratch, 'jetpack'), TMPDIR: scratch, TMP: scratch, TEMP: scratch };
  delete env.JET_COMPILER_EXTENSION; delete env.JET_SCHEMA_CACHE_DIR;
  const call = (binary, args, work = cwd, stdin = '') => invoke(binary, args, work, env, options.timeout, stdin);
  if (entry.suite === 'golden') {
    if (entry.stem === 'devloop/job_runner' || entry.stem.startsWith('lowlevel/polyglot_')) return verdict('REFUSED', 'named job/foreign binder fixture setup still owned by Rust harness');
    const base = join(EXPECTED, entry.stem), expected = { out: optional(`${base}.out`), err: optional(`${base}.err.out`), stderrOut: optional(`${base}.stderr.out`) };
    if (expected.out === null && expected.err === null) return verdict('REFUSED', 'missing semantic service/browser compile-only fact API');
    const built = await call(options.jet, ['build', entry.shown]);
    const failure = processFailure(built);
    if (failure) return failure;
    if (built.code !== 0 || /^Warning \[L0507\]/mu.test(built.stderr)) return { ...verdict('FAIL', 'front end/build failed or L0507'), stdout: built.stdout, stderr: built.stderr };
    const output = built.stdout.match(/^built: (.+)$/mu)?.[1];
    if (!output) return verdict('ERROR', 'CLI build did not report its executable artifact');
    const binary = resolve(cwd, output);
    const run = await call(binary, [], cwd, options.stdin.get(entry.stem) ?? '');
    return { ...(processFailure(run) ?? goldenVerdict(entry, run, expected, binary)), stdout: run.stdout, stderr: run.stderr, exit: run.code };
  }
  const source = readFileSync(entry.path, 'utf8'), plan = uiInvocation(source, entry.shown, entry.suite);
  if (plan.refused) return verdict('REFUSED', plan.refused);
  if (plan.extension) env.JET_COMPILER_EXTENSION = join(ROOT, plan.extension);
  const snapshot = entry.path.slice(0, -4) + '.published.snapshot';
  if (existsSync(snapshot)) {
    const text = readFileSync(snapshot, 'utf8'), type = text.match(/^type = (.+)$/mu)?.[1]?.trim() ?? 'Unknown';
    env.JET_SCHEMA_CACHE_DIR = join(scratch, 'schema'); mkdirSync(env.JET_SCHEMA_CACHE_DIR); writeFileSync(join(env.JET_SCHEMA_CACHE_DIR, `${type}.snapshot`), text);
  }
  const binary = plan.command === 'jetpack' ? options.jetpack : options.jet;
  if (!binary) return verdict('REFUSED', 'jetpack binary required');
  const run = await call(binary, plan.args, plan.cwdParent ? join(cwd, dirname(entry.shown)) : cwd, plan.stdin);
  const failure = processFailure(run); if (failure) return failure;
  let actual;
  if (entry.suite === 'lint') actual = run.stderr;
  else if (!plan.direct) {
    const diagnostic = plan.allDiags ? run.stdout + run.stderr : run.code === 0 ? '' : run.stderr;
    if (run.code !== 0 && !/^(?:Error|Warning|Stop) \[/mu.test(diagnostic)) return verdict('ERROR', 'compiler failed without a rendered diagnostic');
    actual = /^(?:Error|Warning|Stop) \[/mu.test(diagnostic) ? renderedOnly(diagnostic) : '(no errors)\n';
  } else if (plan.args[0] === 'check') actual = run.code === 0 ? '(no errors)\n' : run.stdout + run.stderr;
  else if (plan.args[0] === 'repl') actual = run.stderr;
  else actual = run.stdout + run.stderr;
  actual = normalizeUi(entry.shown, actual, cwd);
  return { ...snapshotVerdict(entry, run, actual, optional(entry.expected)), actual, exit: run.code };
}

export async function main(argv) {
  const options = { suite: 'all', timeout: 600, stdin: loadExampleStdin(), jet: process.env.JET };
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--help') { console.log('test-corpora.mjs --jet BINARY --suite golden|ui|lint|all [--cases FILE] [--filter SUBSTRING] [--results JSON] [--scratch DIR] [--timeout SECONDS] [--rust-audit] [--list]\nNo Cargo or Rust emit invocation. --rust-audit is retired and prints UNCOVERED (exit 2). Nonportable internal fixture APIs are REFUSED, never PASS. Expected files are read-only.'); return; }
    if (argv[i] === '--list') { options.list = true; continue; }
    if (argv[i] === '--rust-audit') { options.rustAudit = true; continue; }
    const key = ({ '--jet': 'jet', '--jetpack': 'jetpack', '--suite': 'suite', '--cases': 'cases', '--filter': 'filter', '--results': 'results', '--scratch': 'scratch', '--timeout': 'timeout' })[argv[i]];
    if (!key || !argv[i + 1]) throw new Error(`unknown or incomplete option ${argv[i]}`);
    options[key] = argv[++i];
  }
  if (options.rustAudit) {
    console.log(JSON.stringify({ status: 'UNCOVERED', cardRequired: true, reason: 'retired generated-Rust I1 audit; native safety is the S12 gate' }));
    process.exitCode = 2;
    return;
  }
  if (!['all', 'golden', 'ui', 'lint'].includes(options.suite)) throw new Error('invalid suite');
  options.timeout = Number(options.timeout); if (!Number.isFinite(options.timeout) || options.timeout <= 0) throw new Error('timeout must be positive');
  let entries = [...(options.suite === 'golden' || options.suite === 'all' ? collectGoldenEntries().map((entry) => ({ ...entry, suite: 'golden' })) : []), ...(options.suite === 'ui' || options.suite === 'all' ? uiEntries() : []), ...(options.suite === 'lint' || options.suite === 'all' ? uiEntries(ROOT, 'lint') : [])];
  if (options.cases) {
    const ids = readFileSync(options.cases, 'utf8').split('\n').map((s) => s.trim()).filter((s) => s && !s.startsWith('#'));
    if (new Set(ids).size !== ids.length) throw new Error('duplicate case IDs');
    const selected = new Set(ids);
    const missing = ids.filter((id) => !entries.some((entry) => entry.shown === id || entry.stem === id));
    if (missing.length) throw new Error(`unknown cases: ${missing.join(', ')}`);
    entries = entries.filter((entry) => selected.has(entry.shown) || selected.has(entry.stem));
  }
  if (options.filter) entries = entries.filter((entry) => entry.shown.includes(options.filter));
  if (!entries.length) throw new Error('no matching corpus entries');
  if (options.list) { for (const entry of entries) console.log(`${entry.suite}\t${entry.shown}`); return; }
  if (!options.jet) throw new Error('--jet must name the admitted compiler binary');
  options.jet = realpathSync(options.jet); if (options.jetpack) options.jetpack = realpathSync(options.jetpack);
  const base = resolve(options.scratch ?? process.env.JET_TEST_SCRATCH_DIR ?? join(process.env.HOME, '.cache/jet-dev/scratch'));
  mkdirSync(base, { recursive: true });
  const real = realpathSync(base);
  if (real === ROOT || real.startsWith(ROOT + sep) || real === '/tmp' || real.startsWith('/tmp/')) throw new Error('scratch must be external and disk-backed');
  options.scratch = mkdtempSync(join(real, 'test-corpora-'));
  const results = [], env = { ...process.env, NO_COLOR: '1', CLICOLOR: '0', TERM: 'dumb', JET_RECEIPT_BYPASS: '1', JET_REPL_HISTORY: 'off' };
  try {
    // Sequential by design; the caller supplies the machine-wide cheap/proofq slot.
    for (const [index, entry] of entries.entries()) {
      const scratch = join(options.scratch, String(index)); mkdirSync(scratch);
      let result;
      try { result = await runEntry(entry, options, scratch, env); } catch (error) { result = verdict('ERROR', error.message); }
      results.push({ suite: entry.suite, case: entry.shown, ...result });
      console.log(`${result.status}\t${entry.shown}\t${result.detail}`);
      rmSync(scratch, { recursive: true, force: true });
    }
    const counts = Object.fromEntries(['PASS', 'FAIL', 'REFUSED', 'ERROR', 'TIMEOUT'].map((status) => [status, results.filter((r) => r.status === status).length]));
    if (options.results) writeFileSync(options.results, JSON.stringify({ jet: options.jet, counts, results }, null, 2) + '\n');
    console.log(JSON.stringify(counts));
    process.exitCode = counts.ERROR || counts.REFUSED || counts.TIMEOUT ? 2 : counts.FAIL ? 1 : 0;
  } finally { rmSync(options.scratch, { recursive: true, force: true }); }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) main(process.argv.slice(2)).catch((error) => { console.error(error.stack); process.exitCode = 2; });
