#!/usr/bin/env node
// preflight-receipt.mjs: the collect-all preflight receipt that admits an expensive launch
// (AGENTS.md, expensive launches are gated).
//
//   node Tools/agent/land/preflight-receipt.mjs identity TREE
//   node Tools/agent/land/preflight-receipt.mjs write SOURCE_IDENTITY SEED_IDENTITY COMMIT [FILE...]
//   node Tools/agent/land/preflight-receipt.mjs check LAUNCHER TREE SEED_DIR
//
// census-snap.sh writes a receipt when every pass of its census is clean: the check, the build, the
// runtime pack and the self-check. The self-hosting chain (chain.sh full), the seed cut
// (release-compiler.mjs) and the release build (publish-release.mjs build) refuse to start without
// one. A receipt is keyed on the tree's compiler source identity (compiler-packages.mjs
// compilerSourceIdentity, the content), so an export matches the census snapshot it reproduces. It
// records the commit and the named worktree files the snapshot was made from, and it admits only a
// launch from the seed whose IDENTITY the census ran. A refusal names the census-snap.sh command to run.
// Receipts: $JET_STATE_ROOT/preflight/<sha256 of the source identity>.json (Tools/agent/roots.mjs).
// Exit: 0 written or admitted; 1 refused; 2 usage or setup failure.
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { compilerSourceIdentity } from '../compiler-packages.mjs';
import { stateRoot } from '../roots.mjs';

const SCRIPT = fileURLToPath(import.meta.url);
const SCHEMA = 'jet.preflight-receipt.v1';
// The census-snap.sh passes a receipt vouches for.
const PASSES = ['check', 'build', 'runtime-pack', 'selfcheck'];
// The trees compilerSourceIdentity reads: the compiler packages, the Jetpack packages they depend on, and Core.
const IDENTITY_TREES = ['Compiler', 'Core', 'Jetpack'];
const USAGE = 'usage: preflight-receipt.mjs identity TREE | write SOURCE_IDENTITY SEED_IDENTITY COMMIT [FILE...] | check LAUNCHER TREE SEED_DIR';

export const receiptDir = () => join(stateRoot(), 'preflight');

export function receiptPath(sourceIdentity, dir = receiptDir()) {
    return join(dir, `${createHash('sha256').update(sourceIdentity).digest('hex')}.json`);
}

export function writeReceipt({ sourceIdentity, seedIdentity, commit, files = [] }, dir = receiptDir()) {
    mkdirSync(dir, { recursive: true });
    const path = receiptPath(sourceIdentity, dir);
    const temp = `${path}.${process.pid}.tmp`;
    const receipt = { schema: SCHEMA, sourceIdentity, seedIdentity, commit, files, passes: PASSES, finished: new Date().toISOString() };
    writeFileSync(temp, `${JSON.stringify(receipt, null, 2)}\n`);
    renameSync(temp, path);
    return path;
}

// Whether a receipt admits launching the tree whose source identity is `sourceIdentity` from the seed
// whose IDENTITY is `seedIdentity`: { admitted, reason }.
export function gate(sourceIdentity, seedIdentity, dir = receiptDir()) {
    const path = receiptPath(sourceIdentity, dir);
    if (!existsSync(path)) return { admitted: false, reason: `no preflight receipt for source identity ${sourceIdentity}` };
    let receipt;
    try { receipt = JSON.parse(readFileSync(path, 'utf8')); } catch (error) { return { admitted: false, reason: `receipt ${path} is not JSON (${error.message})` }; }
    if (receipt.schema !== SCHEMA || receipt.sourceIdentity !== sourceIdentity) return { admitted: false, reason: `receipt ${path} is not a ${SCHEMA} receipt for ${sourceIdentity}` };
    if (receipt.seedIdentity !== seedIdentity) return { admitted: false, reason: `the preflight receipt for source identity ${sourceIdentity} is from a census by seed ${receipt.seedIdentity}, not ${seedIdentity}` };
    const files = receipt.files.length ? ` + ${receipt.files.length} worktree file(s)` : '';
    return { admitted: true, reason: `preflight receipt ${path}: census of ${String(receipt.commit).slice(0, 10)}${files} by seed ${seedIdentity}, finished ${receipt.finished}` };
}

// The source identity of `commit` of the checkout `repo`: an export of the trees the identity reads,
// in a temp directory under `scratch` that is removed again.
export function commitSourceIdentity(repo, commit, scratch = receiptDir()) {
    mkdirSync(scratch, { recursive: true });
    const dir = mkdtempSync(join(scratch, 'gate-'));
    try {
        const archive = spawnSync('bash', ['-c', 'set -o pipefail; git -C "$0" archive "$1" -- "${@:3}" | tar -x -C "$2"', repo, commit, dir, ...IDENTITY_TREES], { encoding: 'utf8' });
        if (archive.status !== 0) throw Error(`git archive ${commit} failed: ${(archive.stderr ?? '').trim()}`);
        return compilerSourceIdentity(dir);
    } finally {
        rmSync(dir, { recursive: true, force: true });
    }
}

// The refusal LAUNCHER prints: the reason, then the census that writes the receipt (run in the dev
// worktree at `commit`, naming every uncommitted file the launch tree carries beyond it).
export function refusal(launcher, reason, seedDir, commit = '') {
    return [
        `${launcher}: refused: ${reason}.`,
        `Run the collect-all preflight in the dev worktree${commit ? ` at ${commit.slice(0, 10)}` : ''} (name each uncommitted file the launch tree carries after it), then launch again:`,
        `  CENSUS_JET_DIR=${seedDir} Tools/agent/land/census-snap.sh`,
    ].join('\n');
}

function main(argv) {
    const [command, ...args] = argv;
    if (command === 'identity' && args.length === 1) {
        process.stdout.write(`${compilerSourceIdentity(resolve(args[0]))}\n`);
        return 0;
    }
    if (command === 'write' && args.length >= 3) {
        const [sourceIdentity, seedIdentity, commit, ...files] = args;
        process.stdout.write(`preflight receipt ${writeReceipt({ sourceIdentity, seedIdentity, commit, files })}\n`);
        return 0;
    }
    if (command === 'check' && args.length === 3) {
        const [launcher, tree, seed] = args;
        const seedDir = resolve(seed);
        if (!existsSync(join(seedDir, 'IDENTITY'))) { process.stderr.write(`${launcher}: seed ${seedDir} has no IDENTITY\n`); return 2; }
        const base = join(tree, '.chain-base');
        const commit = existsSync(base) ? readFileSync(base, 'utf8').split('\n')[0].trim() : '';
        const verdict = gate(compilerSourceIdentity(resolve(tree)), readFileSync(join(seedDir, 'IDENTITY'), 'utf8').trim());
        if (!verdict.admitted) { process.stderr.write(`${refusal(launcher, verdict.reason, seedDir, commit)}\n`); return 1; }
        process.stdout.write(`${launcher}: admitted by ${verdict.reason}\n`);
        return 0;
    }
    process.stderr.write(`${USAGE}\n`);
    return 2;
}

if (process.argv[1] && resolve(process.argv[1]) === SCRIPT) process.exitCode = main(process.argv.slice(2));
