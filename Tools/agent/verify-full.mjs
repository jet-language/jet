#!/usr/bin/env node
// Full native test gate. Independent failures accumulate; no Cargo inventory,
// bootstrap build, reference oracle, skip or blessing can close a suite.
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { homedir } from 'node:os';
import { assertPinnedSource, proofTargetDirectory } from './proof-source.mjs';
import { nativeSuiteCommands, validateNativeSuiteResult } from './native-suite.mjs';
let repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const jet = process.env.JET;
if (!jet) throw Error('JET must name the admitted compiler binary');
const binary = realpathSync(jet);
const parent = resolve(process.env.JET_VERIFY_TMPDIR ?? process.env.TMPDIR ?? join(homedir(), '.cache/jet-dev/scratch'));
mkdirSync(parent, { recursive: true });
const base = realpathSync(parent);
const inside = (root, path) => { const part = relative(root, path); return part === '' || (!isAbsolute(part) && part !== '..' && !part.startsWith(`..${sep}`)); };
if (inside(realpathSync(repo), base) || inside(realpathSync('/tmp'), base)) throw Error('verify-full scratch must be external and disk-backed');
const scratch = mkdtempSync(join(base, 'jet-verify-'));
const evidence = resolve(process.env.JET_CI_EVIDENCE_DIR ?? join(scratch, 'evidence'));
if (inside(realpathSync(repo), evidence)) throw Error('verify-full evidence must be outside the checkout');
mkdirSync(evidence, { recursive: true });
const env = { ...process.env, JET: binary, TMPDIR: scratch, TMP: scratch, TEMP: scratch, JET_TEST_SCRATCH_DIR: scratch, JET_CI_EVIDENCE_DIR: evidence, NO_COLOR: '1', TERM: 'dumb' };
let sourcePin = null;
const rows = [];
let unsafe = '';
const summary = join(evidence, 'verify-full-summary.tsv');
function publish() {
    writeFileSync(summary, 'status\tscope\trc\tprerequisite\tlog\ttarget\n' + rows.map(row => [row.status, row.scope, row.code ?? '-', row.reason ?? '-', row.log, row.scope].join('\t')).join('\n') + '\n');
    writeFileSync(join(evidence, 'verify-full.json'), JSON.stringify({ head: sourcePin, source: repo, jet: binary, scratch, status: rows.length && rows.every(row => row.status === 'PASS') ? 'PASS' : 'FAIL', rows }, null, 2) + '\n');
}
function run(scope, command, args, capture = null) {
    const log = join(evidence, `${scope}.log`);
    if (unsafe) { rows.push({ scope, status: 'BLOCKED', reason: unsafe, log }); publish(); return false; }
    const child = spawnSync(command, args, { cwd: repo, env, encoding: 'utf8', maxBuffer: 128 << 20, timeout: Number(process.env.JET_VERIFY_TIMEOUT_MS ?? 3600000) });
    writeFileSync(log, `${child.stdout ?? ''}${child.stderr ?? ''}`);
    const row = { scope, status: child.status === 0 && !child.error && !child.signal ? 'PASS' : 'FAIL', code: child.status, signal: child.signal, log };
    if (child.error || child.signal || child.status >= 128 || child.status === 124) { unsafe = `unsafe child ${child.signal ?? child.error?.message ?? child.status} in ${scope}`; row.reason = unsafe; }
    rows.push(row); publish();
    if (row.status === 'PASS' && capture) capture(child.stdout);
    return row.status === 'PASS';
}
function checkPin(scope) {
    const log = join(evidence, `${scope}.log`);
    try {
        assertPinnedSource(repo, sourcePin);
        writeFileSync(log, `${sourcePin}\t${repo}\n`);
        rows.push({ scope, status: 'PASS', code: 0, log });
    } catch (error) {
        writeFileSync(log, `${error.stack ?? error}\n`);
        rows.push({ scope, status: 'FAIL', code: 2, reason: error.message, log });
    }
    publish();
    return rows.at(-1).status === 'PASS';
}
// The closeout ballot remains a prerequisite, not a runner escape hatch.
if (!run('closeout-gate', process.execPath, [join(repo, 'Tools/agent/closeout-gate.mjs'), 'check'])) process.exit(1);
const inheritedManifest = join(dirname(repo), 'source.manifest.json');
if (existsSync(inheritedManifest)) {
    sourcePin = JSON.parse(readFileSync(inheritedManifest, 'utf8')).commit;
} else {
    const head = spawnSync('git', ['rev-parse', 'HEAD'], { cwd: repo, encoding: 'utf8' });
    if (head.error || head.status !== 0) throw Error('cannot pin full verification source');
    sourcePin = head.stdout.trim();
    if (!run('source-export', process.execPath, [join(repo, 'Tools/agent/proof-source.mjs'), sourcePin, repo], stdout => { repo = stdout.trim(); })) process.exit(1);
}
if (!checkPin('source-integrity-before')) process.exit(1);
const commit = sourcePin;
env.JET_TOOLCHAIN_ROOT = repo;
env.CARGO_TARGET_DIR = proofTargetDirectory(commit);
env.JET_SCRATCH_ROOT = '/mnt/jetscratch';
if (process.env.JET_NATIVE_TRACE_LOG) {
    env.JET_NATIVE_TRACE_NODE = process.execPath;
    env.JET_NATIVE_TRACE_CACHE_ROOT = join(evidence, 'native-ffi-cache');
    mkdirSync(env.JET_NATIVE_TRACE_CACHE_ROOT, { recursive: true });
}
const shardIndex = process.env.JET_TEST_SHARD ?? '0';
const shardCount = process.env.JET_TEST_SHARD_COUNT ?? '1';
const selection = spawnSync('bash', [join(repo, 'Tools/ci/test-shards.sh'), shardIndex, shardCount], { cwd: repo, env, encoding: 'utf8' });
if (selection.error || selection.status !== 0) throw Error(`native suite inventory failed: ${selection.error ?? selection.stderr}`);
const suites = selection.stdout.trim().split('\n').filter(Boolean);
if (!suites.length || new Set(suites).size !== suites.length || suites.some(suite => !['golden', 'ui', 'cases', 'registries'].includes(suite))) throw Error('invalid or empty native suite inventory');
writeFileSync(join(evidence, 'test-targets.txt'), suites.join('\n') + '\n');
const definitions = nativeSuiteCommands({ root: repo, jet: binary, head: commit, scratch, evidence, suites });
for (const definition of definitions) {
    const missing = definition.dependsOn?.find(scope => rows.find(row => row.scope === scope)?.status !== 'PASS');
    if (missing) {
        rows.push({ scope: definition.scope, status: 'BLOCKED', reason: `prerequisite ${missing} did not pass`, log: join(evidence, `${definition.scope}.log`) });
        publish();
        continue;
    }
    run(definition.scope, definition.command, definition.args);
    const row = rows.at(-1);
    if (row.status === 'PASS') {
        try {
            const documents = Object.fromEntries(Object.entries(definition.documents).map(([name, path]) => [name, JSON.parse(readFileSync(path, 'utf8'))]));
            Object.assign(row, validateNativeSuiteResult(definition, { status: row.code, signal: row.signal }, documents));
        } catch (error) { row.status = 'FAIL'; row.reason = error.message; }
        publish();
    }
}
checkPin('source-integrity-after');
console.error(`verify-full: ${rows.every(row => row.status === 'PASS') ? 'PASS' : 'FAIL'}; complete evidence: ${summary}`);
process.exitCode = rows.every(row => row.status === 'PASS') ? 0 : 1;
