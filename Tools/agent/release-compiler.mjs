#!/usr/bin/env node
// release-compiler.mjs: the release gate of the a0_NNN compiler train.
//
//   node Tools/agent/release-compiler.mjs COMMIT [--summary TEXT] [--dispositions FILE] [--min-gains N]
//        [--shards N] [--jobs N] [--timeout SEC] [--filter SUBSTR] [--seeds DIR] [--work DIR] [--check-gate]
//
// COMMIT is a commit on dev. The current release is `SEEDS/current` (default
// $JET_SEEDS, Tools/agent/roots.mjs). The run starts only with COMMIT's collect-all preflight
// receipt for the current release (`Tools/agent/Land.jet receipt`, written by `Land.jet census`);
// --check-gate says whether it would start and stops. The tool runs every check and
// reports all of them; it does not stop at the first failure:
//   chain     chain.sh builds s1, s2 and s3 of COMMIT from the current release, every stage
//             under norust.sh, and must reach the fixed point (s2 == s3).
//   pack      the runtime pack, rebuilt by s2 without Rust, must equal the chain's pack2 byte
//             for byte.
//   cc-pack   Tools/agent/cc-pack.nix builds this host's C toolchain pack from pinned sources
//             (the only place Nix runs); its SHA-256 and size must equal CC_PACK_ASSETS in
//             Jetpack/Provider/Source/Toolchain/CcToolchainPack.jet, the hash jet embeds.
//   jetpack   Jetpack/Bootstrap/check.sh (keep-going) with s2 on COMMIT must show no failure
//             row the current release showed (recorded when it was cut, else rerun on its own
//             tree), and must finish if the release's census did; `check.sh --hosted` builds
//             the jetpack engine that ships beside jet.
//   suites    the golden, ui and cases test profiles (`jet test Compiler --profile <suite>`,
//             run by s2 with `--compiler` naming the current release, then s2) on COMMIT's
//             fixtures, compared per (suite, case, tier). A row the release
//             passes and s2 fails twice is lost; a lost row needs a disposition line
//             `suite<TAB>case<TAB>tier|*<TAB>#card<TAB>reason` in --dispositions.
//   gains     rows s2 passes that the release fails, plus Jetpack rows s2 no longer shows;
//             at least --min-gains (default 10) of them.
// The suites start only once the cheap checks (chain, pack, jetpack) are
// clean: they are the expensive part (AGENTS.md, expensive launches are gated).
//
// Pass: assembles SEEDS/a0_NNN (the next number) with jet (its Core sources built in), jet-runtime/, jetpack,
// IDENTITY, RELEASE, the C toolchain pack, SOURCE, SHA256SUMS, RELEASE.json, RELEASE.md,
// MERGE_MSG, TAG_MSG and evidence/, then publishes it, each step only after the one before
// succeeds: COMMIT must be on origin/dev; `git tag -a a0_NNN -F TAG_MSG COMMIT`;
// `git push origin a0_NNN`; then the new jet runs `jet run Tools/release/Main.jet -- candidate
// SEEDS/a0_NNN` and `-- publish SEEDS/a0_NNN` from this checkout, signing with the jet-release
// key (`jet registry keygen --registry jet-release` writes it to $JET_KEYS_DIR, default
// ~/.jet/keys; the run does not start without it). Only then does it write
// SEEDS/a0_NNN/current.env and print PASS with the commands left for Main: make it current
// (jet-use) and merge it to master as `Merge dev: a0_NNN <summary>`.
// Fail: no current.env and no PASS. A failed check, or a publication step that fails before
// the tag reaches origin, leaves no SEEDS/a0_NNN (the assembled directory moves to
// SEEDS/.a0_NNN.unpublished-PID and the local tag is deleted); once the tag is on origin,
// SEEDS/a0_NNN stays without current.env and the tool prints the steps left. The evidence
// stays in the work directory. The tool never repoints `current`, merges or pushes master.
// One release run at a time (a lock in the work root); the run takes one
// build-slot.sh slot, and every suite process runs inside that slot's memory cap. Each
// shard's `jet test` gets `--memory` (the slot's SLOT_MEMORY_MAX, 16G when unset, split
// across the shards), because a pool left to size itself reads the host's MemAvailable
// and every shard would claim all of it; --jobs N caps each pool at N entries at once.
// --filter narrows the suites for dry runs of the tool itself: such a run is never a
// release (RELEASE.json scope `filtered`, which jet-use refuses) and is never tagged or
// published.
//
// Environment: the roots of Tools/agent/roots.mjs, JET_RELEASE_WORK (default
// $JET_STATE_ROOT/release), JET_RELEASE_CHAIN (default Tools/agent/chain/chain.sh),
// JET_RELEASE_SLOT (default Tools/agent/chain/build-slot.sh; empty runs without a slot),
// JET_PACK_MUSL_LIB, JET_KEYS_DIR (where the jet-release signing key lives; default ~/.jet/keys).
// Exit: 0 released and published, 1 a check or a publication step failed (not admitted),
// 2 the run could not start.
import { createHash } from 'node:crypto';
import { spawn, spawnSync } from 'node:child_process';
import {
    chmodSync, copyFileSync, cpSync, existsSync, mkdirSync, openSync, closeSync, readFileSync, readdirSync,
    realpathSync, renameSync, rmSync, statSync, writeFileSync, appendFileSync, lstatSync, linkSync,
} from 'node:fs';
import { homedir } from 'node:os';
import { basename, dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { seedsRoot, stateRoot } from './roots.mjs';

const SCRIPT = fileURLToPath(import.meta.url);
const REPO = resolve(dirname(SCRIPT), '../..');
const SUITES = ['golden', 'ui', 'cases'];
const STACK = ['prlimit', '--core=0', '--stack=1073741824:1073741824', '--'];
const MUSL_DEFAULT = '/nix/store/wrng0xbxmz3m7slj6k14vip516wvkjdi-musl-static-x86_64-unknown-linux-musl-1.2.5/lib';
// norust.sh, inline: drop every PATH directory holding cargo, rustc or rustup, refuse if one stays reachable.
const NORUST = 'kept=; IFS=: read -r -a dirs <<< "$PATH"; for d in "${dirs[@]}"; do s=; for b in cargo rustc rustup; do [ -x "$d/$b" ] && s=1; done; [ -n "$s" ] || kept="$kept${kept:+:}$d"; done; export PATH="$kept"; for b in cargo rustc rustup; do if command -v "$b" >/dev/null 2>&1; then echo "norust: REFUSE $b still on PATH" >&2; exit 2; fi; done; exec "$@"';
const USAGE = 'usage: release-compiler.mjs COMMIT [--summary TEXT] [--dispositions FILE] [--min-gains N] [--shards N] [--jobs N] [--timeout SEC] [--filter SUBSTR] [--seeds DIR] [--work DIR] [--check-gate]';
// The registry name of the key that signs a release's CANDIDATE.json (Tools/release/Main.jet RELEASE_KEY).
const RELEASE_KEY = 'jet-release';
// Where `jet registry keygen --registry jet-release` writes the private seed (Jetpack Publication Sign.jet).
function releaseKeyPath() {
    return join(process.env.JET_KEYS_DIR || join(process.env.HOME ?? homedir(), '.jet', 'keys'), `${RELEASE_KEY}.ed25519`);
}
// The pool settings of each of `shards` `jet test` runs sharing one slot: the slot's memory cap split across
// them (each pool alone would size its budget from the whole cgroup room), and `--jobs` only when given; the
// pool's default is one entry per core while their claims fit the budget.
export function shardPool(options, shards, slotMemory = process.env.SLOT_MEMORY_MAX) {
    const gib = Number.parseInt(slotMemory ?? '', 10) > 0 ? Number.parseInt(slotMemory, 10) : 16;
    const pool = [`--memory=${Math.max(1, Math.floor(gib / shards))}G`];
    if (options.jobs > 0) pool.push(`--jobs=${options.jobs}`);
    return pool;
}

// The preflight gate (`Land.jet receipt commit`, run by the seed in `seedDir`): is a launch of
// COMMIT `sha` by that seed admitted by a receipt? `text` is the admission line, or the refusal
// that names the census to run.
export function preflightGate(launcher, seedDir, sha) {
    let identity = '';
    try { identity = readFileSync(join(seedDir, 'IDENTITY'), 'utf8').trim(); } catch { return { admitted: false, text: `${launcher}: seed ${seedDir} has no IDENTITY` }; }
    const run = spawnSync(join(seedDir, 'jet'), ['Tools/agent/Land.jet', 'receipt', 'commit', launcher, REPO, sha, seedDir], {
        cwd: REPO, encoding: 'utf8', maxBuffer: 16 << 20,
        env: { ...process.env, JET_COMPILER_IDENTITY: identity, JET_TOOLCHAIN_ROOT: REPO },
    });
    const text = `${run.stdout ?? ''}${run.stderr ?? ''}`.trim() || `${launcher}: the preflight gate did not run (${run.error?.message ?? `exit ${run.status}`})`;
    return { admitted: run.status === 0, text };
}

// ---- pure helpers (release-compiler.test.mjs) ----

export function parseArgs(argv) {
    const options = { commit: '', summary: '', dispositions: '', minGains: 10, shards: 4, jobs: 0, timeout: 0, filter: '', seeds: '', work: '', checkGate: false, errors: [] };
    const values = new Map([['--summary', 'summary'], ['--dispositions', 'dispositions'], ['--filter', 'filter'], ['--seeds', 'seeds'], ['--work', 'work']]);
    const counts = new Map([['--min-gains', 'minGains'], ['--shards', 'shards'], ['--jobs', 'jobs'], ['--timeout', 'timeout']]);
    for (let i = 0; i < argv.length; i++) {
        const flag = argv[i];
        if (values.has(flag) || counts.has(flag)) {
            const value = argv[++i];
            if (value === undefined) { options.errors.push(`${flag} needs a value`); continue; }
            if (values.has(flag)) options[values.get(flag)] = value;
            else if (!/^\d+$/.test(value)) options.errors.push(`${flag} needs a whole number, not ${value}`);
            else options[counts.get(flag)] = Number(value);
        } else if (flag === '--check-gate') options.checkGate = true;
        else if (flag.startsWith('-')) options.errors.push(`unknown option ${flag}`);
        else if (options.commit) options.errors.push(`one commit, not ${options.commit} and ${flag}`);
        else options.commit = flag;
    }
    if (!options.commit) options.errors.push('COMMIT is required');
    if (options.shards < 1) options.errors.push('--shards must be at least 1');
    return options;
}

// The next release name after every a0_NNN in `names`.
export function nextReleaseName(names) {
    const numbers = names.map(name => /^a0_(\d{3})$/.exec(name)).filter(Boolean).map(match => Number(match[1]));
    const next = (numbers.length ? Math.max(...numbers) : 0) + 1;
    return `a0_${String(next).padStart(3, '0')}`;
}

// The release asset holding one host's seed: jet, its compiler aliases, jetpack, jet-runtime/,
// IDENTITY and SHA256SUMS.
export function seedAssetName(release, platform = process.platform, arch = process.arch) {
    return `jet-${release}-${platform}-${arch}.tar.gz`;
}

// Docs/spec/reference/cc-driver.md: `jet-cc` runs `jet cc`, `jet-cxx` and `jet-c++` run
// `jet c++`. They are hard links of `jet` (a real binary, not a wrapper); the CLI reads
// argv[0] (JetCli Host.jet `jet_cli_alias_args`).
export const COMPILER_ALIASES = ['jet-cc', 'jet-cxx', 'jet-c++'];
// What a seed directory ships (SHA256SUMS covers them; jetpack is absent from some). RELEASE holds the
// release name, which jet uses to download its own C toolchain pack from the release.
export const SEED_FILES = ['jet', ...COMPILER_ALIASES, 'jetpack', 'jet-runtime', 'IDENTITY', 'RELEASE'];

// The jet source that embeds each host's C toolchain pack (file, SHA-256, size) in CC_PACK_ASSETS.
export const CC_PACK_SOURCE = 'Jetpack/Provider/Source/Toolchain/CcToolchainPack.jet';

// The CC_PACK_ASSETS rows of CC_PACK_SOURCE's text: [{ platform, file, sha256, size }].
export function ccPackAssets(text) {
    const start = text.indexOf('CC_PACK_ASSETS:');
    if (start < 0) return [];
    const end = text.indexOf('} }', start);
    const table = text.slice(start, end < 0 ? text.length : end);
    return [...table.matchAll(/\{([^{}]*)\}/g)].map(match => {
        const field = name => new RegExp(`\\b${name}:\\s*"([^"]*)"`).exec(match[1])?.[1] ?? '';
        const size = /\bsize:\s*(\d+)/.exec(match[1]);
        return { platform: field('platform'), file: field('file'), sha256: field('sha256'), size: size ? Number(size[1]) : 0 };
    }).filter(row => row.file);
}

export function linkCompilerAliases(dir) {
    for (const alias of COMPILER_ALIASES) linkSync(join(dir, 'jet'), join(dir, alias));
}

// Disposition lines: suite TAB case TAB tier|* TAB #card TAB reason. A line starting with `#` is a comment.
export function parseDispositions(text) {
    const rows = [], errors = [];
    text.split('\n').forEach((line, index) => {
        if (!line.trim() || line.startsWith('#')) return;
        const [suite, id, tier, card, ...reason] = line.split('\t');
        const why = reason.join('\t').trim();
        if (!SUITES.includes(suite) || !id || !tier || !/^#\d+$/.test(card ?? '') || !why) {
            errors.push(`dispositions line ${index + 1}: want suite<TAB>case<TAB>tier|*<TAB>#card<TAB>reason, got ${JSON.stringify(line)}`);
            return;
        }
        rows.push({ suite, case: id, tier, card, reason: why });
    });
    return { rows, errors };
}

const good = status => status === 'PASS' || status === 'UNEXPECTED-PASS';
// A failing row no known-failing line, refusal or release disposition accounts for: nothing tracks it.
const untracked = status => !good(status) && !['KNOWN-FAILING', 'REFUSED', 'SKIP-HOST', 'OPT-IN'].includes(status);
const rowKey = row => `${row.suite}\t${row.case}\t${row.tier}`;

// Per-row comparison of two testkit result lists over the same fixtures. `retry` holds the
// candidate's second run of the rows it lost the first time. Lost rows carry their disposition.
export function compareSuites(release, candidate, retry = [], dispositions = []) {
    const releaseRows = new Map(release.map(row => [rowKey(row), row]));
    const candidateRows = new Map(candidate.map(row => [rowKey(row), row]));
    const retryRows = new Map(retry.map(row => [rowKey(row), row]));
    const result = { rows: 0, lost: [], gained: [], added: [], flaky: [], untracked: candidate.filter(row => untracked(row.status)), releaseUntracked: release.filter(row => untracked(row.status)).length };
    for (const [key, row] of releaseRows) {
        result.rows++;
        const now = candidateRows.get(key);
        if (!good(row.status) || (now && good(now.status))) {
            if (now && !good(row.status) && good(now.status)) result.gained.push({ ...now, was: row.status });
            continue;
        }
        const again = retryRows.get(key);
        if (again && good(again.status)) { result.flaky.push({ ...row, first: now?.status ?? 'MISSING' }); continue; }
        const lost = { suite: row.suite, case: row.case, tier: row.tier, was: row.status, now: now?.status ?? 'MISSING', detail: now?.detail ?? '' };
        const index = dispositions.findIndex(d => d.suite === row.suite && d.case === row.case && (d.tier === '*' || d.tier === row.tier));
        if (index >= 0) lost.disposition = dispositions[index];
        result.lost.push(lost);
    }
    for (const [key, row] of candidateRows) if (!releaseRows.has(key) && good(row.status)) result.added.push(row);
    // A dispositioned lost row is tracked by its card, so it does not count as untracked.
    const tracked = new Set(result.lost.filter(row => row.disposition).map(rowKey));
    result.untracked = result.untracked.filter(row => !tracked.has(rowKey(row)));
    return result;
}

// Jetpack/Bootstrap check-errors.txt rows as census keys: source file TAB message, line-insensitive
// (the rows census-jetpack.sh compares).
export function jetpackKeys(text) {
    const keys = new Set();
    for (const line of text.split('\n')) {
        // The summary starts at `errors per file:`; a clean check prints only `total errors: 0`.
        if (line.startsWith('errors per file:') || line.startsWith('total errors:')) break;
        const fields = line.split(': ');
        if (fields.length > 1) keys.add(`${fields[0].split(':')[0]}\t${fields[1]}`);
    }
    return [...keys].sort();
}

// chain.sh RESULT (TSV with a header) as rows by stage.
export function parseChainResult(text) {
    const lines = text.split('\n').filter(Boolean);
    const head = (lines.shift() ?? '').split('\t');
    return new Map(lines.map(line => {
        const cells = line.split('\t');
        return [cells[0], Object.fromEntries(head.map((name, i) => [name, cells[i] ?? '']))];
    }));
}

// Paths `git merge-tree --write-tree --name-only` reports as conflicted (after the tree id, up to the blank line).
export function mergeTreeConflicts(text) {
    const lines = text.split('\n');
    const paths = [];
    for (let i = 1; i < lines.length && lines[i] !== ''; i++) paths.push(lines[i]);
    return paths;
}

// ---- process and file helpers ----

function sha256File(path) {
    return createHash('sha256').update(readFileSync(path)).digest('hex');
}

function git(args, options = {}) {
    const result = spawnSync('git', ['-C', REPO, ...args], { encoding: 'utf8', maxBuffer: 256 << 20, ...options });
    return { rc: result.status, out: (result.stdout ?? '').trim(), err: (result.stderr ?? '').trim() };
}

// Every regular file under `root` with its sha256, by relative path.
function fileHashes(root) {
    const out = new Map();
    const walk = dir => {
        for (const name of readdirSync(join(root, dir)).sort()) {
            const rel = dir ? `${dir}/${name}` : name;
            const st = lstatSync(join(root, rel));
            if (st.isDirectory()) walk(rel);
            else if (st.isFile()) out.set(rel, sha256File(join(root, rel)));
        }
    };
    walk('');
    return out;
}

function sameTree(a, b) {
    const left = fileHashes(a), right = fileHashes(b), differ = [];
    for (const [path, hash] of left) if (right.get(path) !== hash) differ.push(path);
    for (const path of right.keys()) if (!left.has(path)) differ.push(path);
    return { differ, files: left.size };
}

let LOG = '';
function say(message) {
    const line = `[release ${new Date().toISOString().slice(11, 19)}] ${message}`;
    process.stderr.write(`${line}\n`);
    if (LOG) appendFileSync(LOG, `${line}\n`);
}

// Run argv to completion with stdout/stderr in files; resolves {rc, timedOut, wall}.
function run(argv, { cwd, env, out, timeout }) {
    const pinned = process.env.SLOT_CPUS ? ['taskset', '-c', process.env.SLOT_CPUS, ...argv] : argv;
    const started = Date.now();
    const fd = openSync(out, 'w');
    return new Promise(done => {
        const child = spawn(pinned[0], pinned.slice(1), { cwd, env, stdio: ['ignore', fd, fd], detached: true });
        let timedOut = false;
        const timer = setTimeout(() => {
            timedOut = true;
            try { process.kill(-child.pid, 'SIGTERM'); } catch {}
            setTimeout(() => { try { process.kill(-child.pid, 'SIGKILL'); } catch {} }, 30000).unref();
        }, timeout * 1000);
        const finish = rc => { clearTimeout(timer); closeSync(fd); done({ rc, timedOut, wall: Math.round((Date.now() - started) / 1000) }); };
        child.on('error', () => finish(127));
        child.on('exit', (code, signal) => finish(code ?? (signal ? 128 : 1)));
    });
}

function tail(path, lines = 8) {
    if (!existsSync(path)) return '';
    return readFileSync(path, 'utf8').split('\n').filter(Boolean).slice(-lines).join(' | ').slice(0, 600);
}

// Test fixtures leave read-only directories behind; make the tree writable before removing it.
function removeTree(dir) {
    if (existsSync(dir)) spawnSync("chmod", ["-R", "u+w", dir]);
    rmSync(dir, { recursive: true, force: true });
}

function exportCommit(sha, dir) {
    rmSync(dir, { recursive: true, force: true });
    mkdirSync(dir, { recursive: true });
    const archive = spawnSync('bash', ['-c', 'set -o pipefail; git -C "$0" archive "$1" | tar -x -C "$2"', REPO, sha, dir], { encoding: 'utf8' });
    if (archive.status !== 0) throw Error(`git archive ${sha} failed: ${archive.stderr}`);
}

// ---- the release ----

function releaseInfo(seeds) {
    const link = join(seeds, 'current');
    if (!existsSync(link)) return { error: `${link} does not exist` };
    const dir = realpathSync(link);
    const missing = ['jet', 'IDENTITY', 'jet-runtime'].filter(name => !existsSync(join(dir, name)));
    if (missing.length) return { error: `current release ${dir} lacks ${missing.join(', ')}` };
    const identity = readFileSync(join(dir, 'IDENTITY'), 'utf8').trim();
    const source = existsSync(join(dir, 'SOURCE')) ? readFileSync(join(dir, 'SOURCE'), 'utf8').trim() : '';
    // The release's own tree, for the release-side suites, is an export of the commit SOURCE names.
    const commit = /^[0-9a-f]{40}\b/.exec(source)?.[0] ?? '';
    if (!commit) return { error: `current release ${dir} names no commit in SOURCE` };
    return {
        dir, name: basename(dir), identity, version: identity.split('#')[0], jet: join(dir, 'jet'), jetSha: sha256File(join(dir, 'jet')),
        jetpack: existsSync(join(dir, 'jetpack')) ? join(dir, 'jetpack') : '', source, commit,
    };
}

function preconditions(options) {
    const errors = [...options.errors];
    const seeds = resolve(options.seeds || seedsRoot());
    const workRoot = resolve(options.work || process.env.JET_RELEASE_WORK || join(stateRoot(), 'release'));
    const chain = process.env.JET_RELEASE_CHAIN ?? join(REPO, 'Tools/agent/chain/chain.sh');
    const musl = process.env.JET_PACK_MUSL_LIB ?? MUSL_DEFAULT;
    let sha = '';
    if (options.commit) {
        const parsed = git(['rev-parse', '--verify', `${options.commit}^{commit}`]);
        if (parsed.rc !== 0) errors.push(`${options.commit} is not a commit of ${REPO}`);
        else {
            sha = parsed.out;
            if (git(['merge-base', '--is-ancestor', sha, 'refs/heads/dev']).rc !== 0) errors.push(`${sha.slice(0, 10)} is not on dev`);
        }
    }
    const release = releaseInfo(seeds);
    if (release.error) errors.push(release.error);
    if (!existsSync(chain)) errors.push(`chain.sh not found at ${chain} (JET_RELEASE_CHAIN)`);
    if (!existsSync(join(musl, 'libc.a'))) errors.push(`no musl libc.a in ${musl} (JET_PACK_MUSL_LIB)`);
    if (!options.filter && !existsSync(releaseKeyPath())) errors.push(`no release signing key at ${releaseKeyPath()}: create it with \`jet registry keygen --registry ${RELEASE_KEY}\``);
    let dispositions = [];
    if (options.dispositions) {
        if (!existsSync(options.dispositions)) errors.push(`dispositions file ${options.dispositions} does not exist`);
        else {
            const parsed = parseDispositions(readFileSync(options.dispositions, 'utf8'));
            errors.push(...parsed.errors);
            dispositions = parsed.rows;
        }
    }
    if (workRoot === seeds || workRoot.startsWith(`${seeds}/`)) errors.push('the work root must be outside the seeds directory');
    // The preflight gate, once the run could otherwise start: COMMIT's receipt from a census by the current release.
    const admission = errors.length ? null : preflightGate('release-compiler', release.dir, sha);
    return { errors, seeds, workRoot, chain, musl, sha, release, dispositions, admission };
}

function takeLock(workRoot) {
    mkdirSync(workRoot, { recursive: true });
    const lock = join(workRoot, 'release.lock');
    for (let attempt = 0; attempt < 2; attempt++) {
        try {
            mkdirSync(lock);
            writeFileSync(join(lock, 'pid'), `${process.pid}\n`);
            return lock;
        } catch {
            const pid = Number(existsSync(join(lock, 'pid')) ? readFileSync(join(lock, 'pid'), 'utf8') : 0);
            let alive = false;
            try { if (pid) { process.kill(pid, 0); alive = true; } } catch {}
            if (alive) return { held: pid };
            rmSync(lock, { recursive: true, force: true });
        }
    }
    return { held: 0 };
}

class Gate {
    constructor() { this.rows = []; }
    row(check, status, detail) { this.rows.push({ check, status, detail }); say(`${check}: ${status} ${detail}`); }
    failed() { return this.rows.filter(row => row.status === 'FAIL' || row.status === 'BLOCKED'); }
}

async function chainStage(ctx, gate) {
    const { work, release } = ctx;
    const out = join(work, 'chain');
    const env = { ...process.env, CHAIN_SEED: release.dir };
    const result = await run(['bash', ctx.chain, ctx.tree, 'full', '--out', out], { cwd: work, env, out: join(work, 'logs/chain.log'), timeout: 4 * 3600 });
    const resultFile = join(out, 'RESULT');
    if (!existsSync(resultFile)) return gate.row('chain', 'FAIL', `chain.sh rc=${result.rc} wrote no RESULT: ${tail(join(work, 'logs/chain.log'))}`);
    const rows = parseChainResult(readFileSync(resultFile, 'utf8'));
    const verdict = rows.get('verdict');
    const problems = [];
    if (verdict?.status !== 'FIXED_POINT') problems.push(`verdict ${verdict?.status ?? 'missing'}`);
    const keys = join(out, 'keys/s1.txt');
    const builder = existsSync(keys) ? /^builder=(\S+)$/m.exec(readFileSync(keys, 'utf8'))?.[1] : '';
    if (builder !== release.jetSha) problems.push(`s1 was built by ${builder || 'unknown'}, not ${release.name} (${release.jetSha.slice(0, 16)})`);
    const s2 = join(out, 's2/jet'), s3 = join(out, 's3/jet');
    if (!existsSync(s2) || !existsSync(s3)) problems.push('s2/jet or s3/jet is missing');
    else if (sha256File(s2) !== sha256File(s3)) problems.push('s2/jet and s3/jet differ');
    if (!existsSync(join(out, 's2/jet-runtime'))) problems.push('s2/jet-runtime (pack2) is missing');
    const stages = [...rows.values()].filter(row => row.stage !== 'verdict').map(row => `${row.stage} ${row.status}`).join(', ');
    if (problems.length) return gate.row('chain', 'FAIL', `${problems.join('; ')} (${stages})`);
    const sha = sha256File(s2);
    // RELEASE.json records both chain stages so Tools/release can check the fixed point from the seed alone.
    const chain = { s2: `${release.version}#${sha}`, s3: `${release.version}#${sha256File(s3)}` };
    ctx.candidate = { jet: s2, runtime: join(out, 's2/jet-runtime'), sha, identity: `${release.version}#${sha}`, jetpack: '', chain };
    gate.row('chain', 'PASS', `fixed point s2 == s3 (jet ${sha.slice(0, 16)}) from ${release.name}, no Rust (${stages}; ${verdict.note})`);
}

async function packStage(ctx, gate) {
    const { work, candidate, tree } = ctx;
    const dir = join(work, 'pack');
    mkdirSync(dir, { recursive: true });
    copyFileSync(candidate.jet, join(dir, 'jet'));
    chmodSync(join(dir, 'jet'), 0o755);
    const env = { ...ctx.env, JET_COMPILER_IDENTITY: candidate.identity, JET_TOOLCHAIN_ROOT: tree };
    // The pack compiles the runtime's vendored C with a host C toolchain, which only jet-env supplies until jet does.
    const argv = [...STACK, join(tree, 'Tools/agent/jet-env'), 'bash', '-c', NORUST, 'norust', 'node', join(tree, 'Tools/packaging/prebuild-runtime.mjs'), join(dir, 'jet'), join(dir, 'jet-runtime'), '--repo', tree];
    const result = await run(argv, { cwd: tree, env, out: join(work, 'logs/pack.log'), timeout: 3600 });
    if (result.rc !== 0 || !existsSync(join(dir, 'jet-runtime'))) return gate.row('pack', 'FAIL', `the pack rebuild by s2 failed rc=${result.rc}: ${tail(join(work, 'logs/pack.log'))}`);
    const { differ, files } = sameTree(join(dir, 'jet-runtime'), candidate.runtime);
    if (differ.length) return gate.row('pack', 'FAIL', `the rebuilt pack differs from pack2 in ${differ.length} file(s): ${differ.slice(0, 8).join(', ')}`);
    gate.row('pack', 'PASS', `rebuilt by s2 without Rust, byte-identical to pack2 (${files} files, ${result.wall}s)`);
}

// Why the pack built from pinned sources cannot ship with this jet, or '' when it can: its SHA-256 and
// size must be the ones jet embeds, because jet refuses any other pack.
export function ccPackProblem(expected, built) {
    if (built.sha256 === expected.sha256 && built.size === expected.size) return '';
    return `the C toolchain pack built from pinned sources is sha256 ${built.sha256} (${built.size} bytes), but jet embeds ${expected.sha256} (${expected.size} bytes) for ${expected.file}: update CC_PACK_ASSETS in ${CC_PACK_SOURCE}`;
}

// Builds this host's C toolchain pack with Tools/agent/cc-pack.nix (Nix runs only on the release
// machine) and checks it against the hash jet embeds; assemble() ships it beside jet.
async function ccPackStage(ctx, gate) {
    const { work, tree } = ctx;
    const platform = `${process.platform}-${process.arch}`;
    const expected = ccPackAssets(readFileSync(join(tree, CC_PACK_SOURCE), 'utf8')).find(row => row.platform === platform);
    if (!expected) return gate.row('cc-pack', 'FAIL', `${CC_PACK_SOURCE} embeds no C toolchain pack for ${platform}`);
    const link = join(work, 'cc-pack-result'), log = join(work, 'logs/cc-pack.log');
    const result = await run(['nix-build', join(tree, 'Tools/agent/cc-pack.nix'), '--out-link', link], { cwd: tree, env: ctx.env, out: log, timeout: 12 * 3600 });
    const path = join(link, expected.file);
    if (result.rc !== 0 || !existsSync(path)) return gate.row('cc-pack', 'FAIL', `nix-build Tools/agent/cc-pack.nix failed rc=${result.rc}: ${tail(log)}`);
    const problem = ccPackProblem(expected, { sha256: sha256File(path), size: statSync(path).size });
    if (problem) return gate.row('cc-pack', 'FAIL', problem);
    ctx.ccPack = { path, file: expected.file };
    gate.row('cc-pack', 'PASS', `${expected.file} rebuilt from pinned sources matches the hash jet embeds (${expected.sha256.slice(0, 16)}, ${expected.size} bytes, ${result.wall}s)`);
}

function bootstrapScratch(worker) {
    const root = join(stateRoot(), 'jetpack-bootstrap');
    return existsSync(root) ? readdirSync(root).filter(name => name.startsWith(`${worker}-`)).map(name => join(root, name)) : [];
}

// One keep-going Jetpack/Bootstrap check with `jet` on the tree `root`; resolves its census keys or null.
async function jetpackCensus(ctx, side, jet, identity, root) {
    const worker = `release-${side}-${process.pid}`;
    for (const dir of bootstrapScratch(worker)) rmSync(dir, { recursive: true, force: true });
    const env = { ...ctx.env, JET_KEEP_GOING: '1', JET_COMPILER_IDENTITY: identity, JET_TOOLCHAIN_ROOT: root, JETPACK_WORKER: worker, JET: jet };
    const log = join(ctx.work, `logs/jetpack-${side}.log`);
    const result = await run([...STACK, 'bash', 'Jetpack/Bootstrap/check.sh'], { cwd: root, env, out: log, timeout: 3600 });
    const errors = bootstrapScratch(worker).map(dir => join(dir, 'check-errors.txt')).find(existsSync);
    const receipt = bootstrapScratch(worker).map(dir => join(dir, 'check.receipt')).find(existsSync);
    // check.sh writes the receipt's `exit:` line last: without it the census is partial.
    const finished = !result.timedOut && receipt && /^exit: \d+$/m.test(readFileSync(receipt, 'utf8'));
    const keys = finished && errors ? jetpackKeys(readFileSync(errors, 'utf8')) : [];
    for (const dir of bootstrapScratch(worker)) rmSync(dir, { recursive: true, force: true });
    if (!finished) return { keys: null, detail: `check.sh did not finish (rc=${result.rc}${result.timedOut ? ', timed out' : ''}): ${tail(log)}` };
    return { keys, detail: `rc=${result.rc}, ${keys.length} rows, ${result.wall}s` };
}

// The release's census: the rows it recorded when it was cut (evidence/jetpack-candidate.keys), else a
// census of the release on its own tree. The candidate must add no row; a census that finishes for
// the release must finish for the candidate.
async function jetpackStage(ctx, gate) {
    const { release, candidate } = ctx;
    const recorded = join(release.dir, 'evidence/jetpack-candidate.keys');
    const cache = join(ctx.workRoot, 'cache', `jetpack-${createHash('sha256').update(`${release.jetSha}\0${release.commit}`).digest('hex').slice(0, 16)}.keys`);
    const readKeys = path => readFileSync(path, 'utf8').split('\n').filter(Boolean);
    let before;
    if (existsSync(recorded)) before = { keys: readKeys(recorded), detail: `recorded in ${release.name}/evidence` };
    else if (existsSync(cache)) before = { keys: readKeys(cache), detail: `cached ${basename(cache)}` };
    else {
        before = await jetpackCensus(ctx, 'release', release.jet, release.identity, ctx.releaseTree);
        if (before.keys) { mkdirSync(dirname(cache), { recursive: true }); writeFileSync(cache, before.keys.map(key => `${key}\n`).join('')); }
    }
    const after = await jetpackCensus(ctx, 'candidate', candidate.jet, candidate.identity, ctx.tree);
    if (before.keys) writeFileSync(join(ctx.work, 'evidence/jetpack-release.keys'), before.keys.map(key => `${key}\n`).join(''));
    if (after.keys) writeFileSync(join(ctx.work, 'evidence/jetpack-candidate.keys'), after.keys.map(key => `${key}\n`).join(''));
    if (!before.keys && !after.keys) {
        ctx.jetpackSummary = `Jetpack census finishes on neither ${release.name} nor s2`;
        return gate.row('jetpack', 'NOTE', `the census finishes on neither side, so nothing regressed (release: ${before.detail}; candidate: ${after.detail})`);
    }
    if (!after.keys) return gate.row('jetpack', 'FAIL', `the census finishes for ${release.name} (${before.detail}) but not for the candidate: ${after.detail}`);
    if (!before.keys) {
        ctx.jetpackSummary = `Jetpack census restored (${after.keys.length} rows; ${release.name} could not finish it)`;
        return gate.row('jetpack', 'NOTE', `census restored: ${release.name} could not finish it (${before.detail}); candidate ${after.detail}`);
    }
    const old = new Set(before.keys), now = new Set(after.keys);
    const added = after.keys.filter(key => !old.has(key));
    ctx.jetpackFixed = before.keys.filter(key => !now.has(key));
    if (added.length) return gate.row('jetpack', 'FAIL', `${added.length} new census row(s): ${added.slice(0, 6).map(key => key.replace('\t', ': ')).join(' | ')}`);
    ctx.jetpackSummary = `Jetpack census 0 new rows, ${ctx.jetpackFixed.length} fixed`;
    gate.row('jetpack', 'PASS', `no new census rows (${after.keys.length} rows, ${ctx.jetpackFixed.length} fixed; release ${before.detail}; candidate ${after.detail})`);
}

async function hostedStage(ctx, gate) {
    const { candidate, tree, work } = ctx;
    const worker = `release-hosted-${process.pid}`;
    for (const dir of bootstrapScratch(worker)) rmSync(dir, { recursive: true, force: true });
    const env = { ...ctx.env, JET_COMPILER_IDENTITY: candidate.identity, JET_TOOLCHAIN_ROOT: tree, JETPACK_WORKER: worker, JET: candidate.jet };
    const log = join(work, 'logs/jetpack-hosted.log');
    const result = await run([...STACK, 'bash', '-c', NORUST, 'norust', 'bash', 'Jetpack/Bootstrap/check.sh', '--hosted'], { cwd: tree, env, out: log, timeout: 3 * 3600 });
    const receipt = bootstrapScratch(worker).map(dir => join(dir, 'hosted.receipt')).find(existsSync);
    const text = receipt ? readFileSync(receipt, 'utf8') : '';
    const binary = /^hosted-binary: (.+)$/m.exec(text)?.[1];
    const ok = /^exit: 0$/m.test(text) && binary && existsSync(binary);
    if (ok) {
        mkdirSync(join(work, 'jetpack'), { recursive: true });
        copyFileSync(binary, join(work, 'jetpack/jetpack'));
        chmodSync(join(work, 'jetpack/jetpack'), 0o755);
        candidate.jetpack = join(work, 'jetpack/jetpack');
        // The seed ships jetpack beside jet, and jet looks for its engine beside itself first; give s2 the
        // same layout so the suites test what ships rather than whatever `jetpack` PATH resolves first.
        copyFileSync(binary, join(dirname(candidate.jet), 'jetpack'));
        chmodSync(join(dirname(candidate.jet), 'jetpack'), 0o755);
    }
    for (const dir of bootstrapScratch(worker)) rmSync(dir, { recursive: true, force: true });
    if (ok) return gate.row('jetpack-engine', 'PASS', `check.sh --hosted built jetpack with s2 without Rust (${sha256File(candidate.jetpack).slice(0, 16)}, ${result.wall}s)`);
    const why = `check.sh --hosted rc=${result.rc}${result.timedOut ? ' (timed out)' : ''}: ${tail(log)}`;
    // Losing the engine the current release ships is a regression; a release that never had one only notes it.
    if (ctx.release.jetpack) gate.row('jetpack-engine', 'FAIL', `${ctx.release.name} ships jetpack and s2 cannot build it: ${why}`);
    else gate.row('jetpack-engine', 'NOTE', `no jetpack engine (${ctx.release.name} ships none either): ${why}`);
}

// One side's run of one suite: --shards `jet test Compiler --profile <suite>` processes, merged. s2 runs
// every one, and `--compiler` names the side's jet that builds and runs the rows; the jetpack beside that
// jet serves them. Resolves {rows} or {error}.
async function suiteRun(ctx, side, suite, extra = []) {
    const compiler = side === 'release'
        ? { jet: ctx.release.jet, identity: ctx.release.identity, root: ctx.releaseTree }
        : { jet: ctx.candidate.jet, identity: ctx.candidate.identity, root: ctx.tree };
    const retry = extra.length > 0;
    const shards = retry ? 1 : ctx.options.shards;
    const runs = [];
    for (let index = 1; index <= shards; index++) {
        const tag = `${side}-${suite}${retry ? '-retry' : ''}-${index}`;
        const env = { ...ctx.env, JET_COMPILER_IDENTITY: compiler.identity, JET_TOOLCHAIN_ROOT: compiler.root, JET: compiler.jet };
        // Suite rows run host tools (C compilers, rustc, interpreters) that only jet-env supplies until jet does.
        const argv = [...STACK, join(ctx.tree, 'Tools/agent/jet-env'), ctx.candidate.jet, 'test', join(ctx.tree, 'Compiler'), '--profile', suite, `--compiler=${compiler.jet}`, `--results=${join(ctx.work, 'kit', `${tag}.json`)}`];
        if (!retry) argv.push(`--shard=${index}/${shards}`);
        argv.push(...shardPool(ctx.options, shards));
        if (ctx.options.filter && !retry) argv.push(`--filter=${ctx.options.filter}`);
        if (ctx.options.timeout) argv.push(`--time-limit=${ctx.options.timeout}s`);
        argv.push(...extra);
        runs.push(run(argv, { cwd: ctx.tree, env, out: join(ctx.work, 'kit', `${tag}.log`), timeout: 12 * 3600 }).then(result => ({ tag, result })));
    }
    const rows = [], errors = [];
    for (const { tag, result } of await Promise.all(runs)) {
        const file = join(ctx.work, 'kit', `${tag}.json`);
        const log = join(ctx.work, 'kit', `${tag}.log`);
        // The kit refuses an empty selection (a --filter or a shard that matches nothing): zero rows.
        if (!existsSync(file) && result.rc === 2 && /no matching corpus entries/.test(readFileSync(log, 'utf8'))) continue;
        if (!existsSync(file)) { errors.push(`${tag} rc=${result.rc}${result.timedOut ? ' (timed out)' : ''} wrote no results: ${tail(log, 3)}`); continue; }
        rows.push(...JSON.parse(readFileSync(file, 'utf8')).results);
    }
    return errors.length ? { error: errors.join('; '), rows } : { rows };
}

async function suitesStage(ctx, gate) {
    ctx.suites = {};
    for (const suite of SUITES) {
        const key = createHash('sha256').update([ctx.release.jetSha, ctx.release.identity, ctx.release.jetpack ? sha256File(ctx.release.jetpack) : '', ctx.sha, suite, ctx.options.filter, ctx.options.timeout].join('\0')).digest('hex').slice(0, 16);
        const cache = join(ctx.workRoot, 'cache', `kit-${suite}-${key}.json`);
        let release;
        if (existsSync(cache)) release = { rows: JSON.parse(readFileSync(cache, 'utf8')) };
        else {
            release = await suiteRun(ctx, 'release', suite);
            if (!release.error) { mkdirSync(dirname(cache), { recursive: true }); writeFileSync(cache, JSON.stringify(release.rows)); }
        }
        const candidate = await suiteRun(ctx, 'candidate', suite);
        writeFileSync(join(ctx.work, `evidence/kit-release-${suite}.json`), JSON.stringify(release.rows));
        writeFileSync(join(ctx.work, `evidence/kit-candidate-${suite}.json`), JSON.stringify(candidate.rows));
        if (release.error || candidate.error) { gate.row(`suite:${suite}`, 'BLOCKED', [release.error && `release: ${release.error}`, candidate.error && `candidate: ${candidate.error}`].filter(Boolean).join('; ')); continue; }
        let compared = compareSuites(release.rows, candidate.rows, [], ctx.dispositions);
        let retried = [];
        if (compared.lost.length) {
            const ids = join(ctx.work, 'kit', `retry-${suite}.txt`);
            writeFileSync(ids, [...new Set(compared.lost.map(row => row.case))].map(id => `${id}\n`).join(''));
            const again = await suiteRun(ctx, 'candidate', suite, [`--cases=${ids}`]);
            retried = again.rows;
            compared = compareSuites(release.rows, candidate.rows, retried, ctx.dispositions);
        }
        ctx.suites[suite] = compared;
        const open = compared.lost.filter(row => !row.disposition);
        const summary = `${compared.rows} rows: ${compared.gained.length} gained, ${compared.lost.length} lost (${compared.lost.length - open.length} dispositioned), ${compared.added.length} new passing, ${compared.flaky.length} passed on retry`;
        if (open.length) gate.row(`suite:${suite}`, 'FAIL', `${summary}; lost without a disposition: ${open.slice(0, 6).map(row => `${row.case} ${row.tier} ${row.was}->${row.now}`).join(' | ')}`);
        else gate.row(`suite:${suite}`, 'PASS', summary);
        // Untracked failures may only shrink: every one is listed in REPORT.md, and a release with more fails.
        const count = compared.untracked.length, before = compared.releaseUntracked;
        gate.row(`untracked:${suite}`, count > before ? 'FAIL' : 'PASS', `${count} failing row(s) with no known-failing line (release ${before}); full list in REPORT.md`);
    }
    const unused = ctx.dispositions.filter(d => !Object.values(ctx.suites).some(s => s.lost.some(row => row.disposition === d)));
    if (unused.length) gate.row('dispositions', 'NOTE', `${unused.length} disposition(s) match no lost row: ${unused.slice(0, 4).map(d => `${d.suite} ${d.case} ${d.tier} ${d.card}`).join(' | ')}`);
}

function mergePlan(ctx) {
    const plan = { checkout: '', conflicts: [], resolutions: [], fastForward: false, origin: '' };
    const worktrees = git(['worktree', 'list', '--porcelain']).out.split('\n\n');
    plan.checkout = worktrees.map(block => ({ path: /^worktree (.+)$/m.exec(block)?.[1], branch: /^branch (.+)$/m.exec(block)?.[1] })).find(w => w.branch === 'refs/heads/master')?.path ?? '';
    const merged = git(['merge-tree', '--write-tree', '--name-only', 'refs/heads/master', ctx.sha]);
    if (merged.rc === 1) plan.conflicts = mergeTreeConflicts(merged.out);
    else if (merged.rc !== 0) plan.error = `git merge-tree failed: ${merged.err}`;
    for (const path of plan.conflicts) {
        // Dev already carries master's version when that blob appears in dev's history of the path:
        // the dev side is the newer edit of the same text.
        const ours = git(['rev-parse', '--verify', '-q', `refs/heads/master:${path}`]).out;
        const history = git(['log', '--format=%H', ctx.sha, '--', path]).out.split('\n').filter(Boolean);
        const carried = ours && history.some(commit => git(['rev-parse', '--verify', '-q', `${commit}:${path}`]).out === ours);
        plan.resolutions.push({ path, take: carried ? 'dev' : 'by hand' });
    }
    plan.origin = git(['rev-parse', '--verify', '-q', 'refs/remotes/origin/master']).out;
    plan.fastForward = !plan.origin || git(['merge-base', '--is-ancestor', plan.origin, ctx.sha]).rc === 0;
    return plan;
}

function gainsOf(ctx) {
    const suites = ctx.suites ?? {};
    const gained = Object.values(suites).reduce((n, s) => n + s.gained.length, 0);
    return { suites: gained, jetpack: (ctx.jetpackFixed ?? []).length, total: gained + (ctx.jetpackFixed ?? []).length };
}

function report(ctx, gate, verdict, plan) {
    const gains = gainsOf(ctx);
    const lines = [`# ${ctx.next}: release gate of dev ${ctx.sha.slice(0, 10)}`, '', `Verdict: ${verdict}${ctx.options.filter ? ` (filtered by "${ctx.options.filter}": not a release)` : ''}`, `Seed: ${ctx.release.name} (${ctx.release.identity})`];
    if (ctx.candidate) lines.push(`Candidate: ${ctx.candidate.identity}`);
    lines.push('', '| check | status | detail |', '|---|---|---|', ...gate.rows.map(row => `| ${row.check} | ${row.status} | ${row.detail.replace(/\|/g, '\\|')} |`));
    lines.push('', `Gains: ${gains.total} (${gains.suites} suite rows, ${gains.jetpack} Jetpack census rows)`);
    for (const [suite, s] of Object.entries(ctx.suites ?? {})) {
        if (s.gained.length) lines.push('', `## ${suite}: gained`, ...s.gained.slice(0, 200).map(row => `- ${row.case} ${row.tier}: ${row.was} -> ${row.status}`));
        if (s.lost.length) lines.push('', `## ${suite}: lost`, ...s.lost.map(row => `- ${row.case} ${row.tier}: ${row.was} -> ${row.now}${row.disposition ? ` (${row.disposition.card}: ${row.disposition.reason})` : ' (no disposition)'}`));
        if (s.untracked?.length) lines.push('', `## ${suite}: untracked failures`, ...s.untracked.map(row => `- ${row.case} ${row.tier}: ${row.status} ${String(row.detail ?? '').split('\n')[0].slice(0, 160)}`));
    }
    if (ctx.jetpackFixed?.length) lines.push('', '## Jetpack census: fixed', ...ctx.jetpackFixed.slice(0, 200).map(key => `- ${key.replace('\t', ': ')}`));
    if (plan) {
        lines.push('', '## Master merge', `- predicted conflicts: ${plan.conflicts.length ? plan.resolutions.map(r => `${r.path} (take ${r.take})`).join(', ') : 'none'}`);
        lines.push(`- push: ${plan.fastForward ? 'fast-forwards origin/master' : `origin/master ${plan.origin.slice(0, 10)} is not an ancestor of ${ctx.sha.slice(0, 10)}; the push would not fast-forward`}`);
    }
    return `${lines.join('\n')}\n`;
}

function sumsOf(dir, names) {
    const lines = [];
    for (const name of names) {
        const path = join(dir, name);
        if (!existsSync(path)) continue;
        if (statSync(path).isDirectory()) for (const [rel, hash] of fileHashes(path)) lines.push(`${hash}  ${name}/${rel}`);
        else lines.push(`${sha256File(path)}  ${name}`);
    }
    return `${lines.join('\n')}\n`;
}

// The chain and seed helpers publish-release.mjs shares.
export { MUSL_DEFAULT, NORUST, STACK, fileHashes, sha256File, sumsOf };

function assemble(ctx, gate, plan) {
    const { seeds, next, sha, candidate, release } = ctx;
    const gains = gainsOf(ctx);
    const lost = Object.values(ctx.suites).flatMap(s => s.lost);
    const date = new Date().toISOString().slice(0, 10);
    const scope = ctx.options.filter ? `filtered: ${ctx.options.filter}` : 'full';
    const summary = ctx.options.summary || `${gains.total} gains over ${release.name}`;
    const dest = join(seeds, next), temp = join(seeds, `.${next}.assembling-${process.pid}`);
    rmSync(temp, { recursive: true, force: true });
    mkdirSync(temp);
    copyFileSync(candidate.jet, join(temp, 'jet'));
    chmodSync(join(temp, 'jet'), 0o755);
    linkCompilerAliases(temp);
    cpSync(candidate.runtime, join(temp, 'jet-runtime'), { recursive: true, verbatimSymlinks: true });
    if (candidate.jetpack) { copyFileSync(candidate.jetpack, join(temp, 'jetpack')); chmodSync(join(temp, 'jetpack'), 0o755); }
    const verified = `self-host chain from ${release.name} without Rust reaches the fixed point s2 == s3 (jet ${candidate.sha.slice(0, 16)}); runtime pack rebuilt by s2 is byte-identical; golden/ui/cases ${gains.suites} rows gained, ${lost.length} lost${lost.length ? ` (${[...new Set(lost.map(row => row.disposition.card))].join(', ')})` : ''}; ${ctx.jetpackSummary}${candidate.jetpack ? '; jetpack engine built by s2' : ''}`;
    writeFileSync(join(temp, 'IDENTITY'), `${candidate.identity}\n`);
    writeFileSync(join(temp, 'RELEASE'), `${next}\n`);
    // The C toolchain pack sits beside jet, where jet looks first; it stays out of SHA256SUMS
    // because jet checks it against the hash it embeds.
    copyFileSync(ctx.ccPack.path, join(temp, ctx.ccPack.file));
    chmodSync(join(temp, ctx.ccPack.file), 0o644);
    writeFileSync(join(temp, 'SOURCE'), `${sha} (dev, ${next}: self-hosted s2 of ${sha.slice(0, 9)} from seed ${release.name}; ${verified}; release-compiler.mjs ${date})\n`);
    // jet-use installs a release from its current.env, so it waits in the work directory and
    // reaches dest only once the release is published (release()).
    writeFileSync(join(ctx.work, 'current.env'), [
        '# Newest verified self-hosted compiler (written by release-compiler.mjs; jet-use installs it).',
        `LABEL="${next} (dev ${sha.slice(0, 9)}, ${date}, verified: s2 == s3 ${candidate.sha.slice(0, 16)}, no Rust; ${gains.total} gains, ${lost.length} lost over ${release.name})"`,
        `JET=${dest}/jet`, `JET_IDENTITY=${candidate.identity}`, '',
    ].join('\n'));
    const body = [`Verified ${sha.slice(0, 10)}: ${verified}.`, `Identity ${candidate.identity}.`, `Evidence: ${dest}/RELEASE.md`].join('\n\n');
    writeFileSync(join(temp, 'MERGE_MSG'), `Merge dev: ${next} ${summary}\n\n${body}\n`);
    writeFileSync(join(temp, 'TAG_MSG'), `${next}: ${summary}\n\n${body}\n`);
    cpSync(join(ctx.work, 'evidence'), join(temp, 'evidence'), { recursive: true, verbatimSymlinks: true });
    for (const name of ['RESULT', 'REPORT.md']) if (existsSync(join(ctx.work, 'chain', name))) copyFileSync(join(ctx.work, 'chain', name), join(temp, 'evidence', `chain-${name}`));
    writeFileSync(join(temp, 'RELEASE.json'), `${JSON.stringify({
        schema: 'jet-release/v1', name: next, commit: sha, verdict: 'PASS', scope, created: new Date().toISOString(),
        identity: candidate.identity, target: `${process.platform}-${process.arch}`, chain: candidate.chain, seed: { name: release.name, identity: release.identity }, summary, gains,
        checks: gate.rows, lost: lost.map(({ suite, case: id, tier, was, now, disposition }) => ({ suite, case: id, tier, was, now, card: disposition.card, reason: disposition.reason })),
        merge: plan,
    }, null, 2)}\n`);
    writeFileSync(join(temp, 'RELEASE.md'), report(ctx, gate, 'PASS', plan));
    writeFileSync(join(temp, 'SHA256SUMS'), sumsOf(temp, SEED_FILES));
    if (existsSync(dest)) throw Error(`${dest} appeared while assembling; ${temp} is left for inspection`);
    renameSync(temp, dest);
    return { dest, summary, scope };
}

// The publication of an assembled release (the header's Pass): each step runs only after the one
// before succeeds, its output in logs/publish.log. The new jet runs Tools/release/Main.jet from this
// checkout, and is the `jet` that tool finds on PATH.
function publish(ctx, released) {
    const { dest } = released;
    const log = join(ctx.work, 'logs', 'publish.log');
    const env = { ...process.env, PATH: `${dest}:${process.env.PATH ?? ''}`, JET_COMPILER_IDENTITY: ctx.candidate.identity };
    for (const name of ['JET', 'JET_TOOLCHAIN_ROOT']) delete env[name];
    const release = [join(dest, 'jet'), 'run', 'Tools/release/Main.jet', '--'];
    const steps = [
        ['fetch origin dev', ['git', '-C', REPO, 'fetch', '--quiet', 'origin', '+refs/heads/dev:refs/remotes/origin/dev']],
        ['on origin/dev', ['git', '-C', REPO, 'merge-base', '--is-ancestor', ctx.sha, 'refs/remotes/origin/dev']],
        ['tag', ['git', '-C', REPO, 'tag', '-a', ctx.next, '-F', join(dest, 'TAG_MSG'), ctx.sha]],
        ['push tag', ['git', '-C', REPO, 'push', 'origin', ctx.next]],
        ['candidate', [...release, 'candidate', dest]],
        ['publish', [...release, 'publish', dest]],
    ];
    const state = { ok: false, tagged: false, pushed: false, step: '', detail: '', log, left: [] };
    for (const [index, [step, argv]] of steps.entries()) {
        say(`publish: ${argv.join(' ')}`);
        appendFileSync(log, `$ ${argv.join(' ')}\n`);
        const fd = openSync(log, 'a');
        const result = spawnSync(argv[0], argv.slice(1), { cwd: REPO, env, stdio: ['ignore', fd, fd] });
        closeSync(fd);
        if (result.status !== 0) {
            state.step = step;
            state.detail = step === 'on origin/dev' ? `${ctx.sha.slice(0, 10)} is not on origin/dev; push dev first` : `exit ${result.status ?? result.signal ?? result.error?.code}: ${tail(log, 3)}`;
            state.left = steps.slice(index).map(([, args]) => args.join(' '));
            return state;
        }
        if (step === 'tag') state.tagged = true;
        if (step === 'push tag') state.pushed = true;
    }
    state.ok = true;
    return state;
}

function printCommands(ctx, released, plan) {
    const { dest } = released;
    const lines = [];
    if (ctx.options.filter) lines.push(`FILTERED RUN (--filter ${ctx.options.filter}): ${ctx.next} is not a release and was neither tagged nor published. Do not run these commands; jet-use refuses it.`, '');
    else lines.push(`PASS: ${dest} is admitted: tag ${ctx.next} is on origin and published as its GitHub release; current still points at ${ctx.release.name}. Commands for Main:`, '');
    lines.push(`  jet-use ${ctx.next}`);
    lines.push(`  # without jet-use: ln -sfn ${ctx.next} ${ctx.seeds}/current.new && mv -T ${ctx.seeds}/current.new ${ctx.seeds}/current && cp ${dest}/current.env ${ctx.seeds}/current.env`);
    lines.push(`  cd ${plan.checkout || '<the checkout of master>'}`);
    lines.push(`  git merge --no-ff -F ${dest}/MERGE_MSG ${ctx.sha}`);
    if (plan.conflicts.length) {
        for (const r of plan.resolutions) lines.push(r.take === 'dev' ? `  git checkout --theirs -- '${r.path}' && git add -- '${r.path}'   # dev already carries master's version and edits it later` : `  # resolve '${r.path}' by hand, then git add it`);
        lines.push('  git commit --no-edit');
    }
    if (plan.fastForward) lines.push('  git push origin master');
    else lines.push(`  # do not push: origin/master ${plan.origin.slice(0, 10)} is not an ancestor of ${ctx.sha.slice(0, 10)}, so the push would not fast-forward. Release a newer dev commit instead; never force-push master.`);
    process.stdout.write(`${lines.join('\n')}\n`);
}

async function release(options, pre) {
    const { seeds, workRoot, sha, release } = pre;
    const work = join(workRoot, sha.slice(0, 12));
    removeTree(work);
    for (const dir of ['logs', 'evidence', 'kit', 'tmp']) mkdirSync(join(work, dir), { recursive: true });
    LOG = join(work, 'release.log');
    const env = { ...process.env, JET_PACK_MUSL_LIB: pre.musl, TMPDIR: join(work, 'tmp') };
    delete env.JET;
    const ctx = { options, seeds, workRoot, work, sha, release, chain: pre.chain, env, dispositions: pre.dispositions, next: nextReleaseName(readdirSync(seeds)), tree: join(work, 'tree'), releaseTree: join(work, 'release-tree') };
    const gate = new Gate();
    say(`release gate of ${sha.slice(0, 10)} against ${release.name}; next name ${ctx.next}; work ${work}`);
    exportCommit(sha, ctx.tree);
    writeFileSync(join(ctx.tree, '.chain-base'), `${sha}\n`);
    exportCommit(release.commit, ctx.releaseTree);
    await chainStage(ctx, gate);
    await ccPackStage(ctx, gate);
    if (ctx.candidate) {
        await packStage(ctx, gate);
        await jetpackStage(ctx, gate);
        await hostedStage(ctx, gate);
    } else for (const check of ['pack', 'jetpack', 'jetpack-engine']) gate.row(check, 'BLOCKED', 'no s2: the chain failed');
    writeFileSync(join(work, 'preflight.json'), `${JSON.stringify({ commit: sha, rows: gate.rows }, null, 2)}\n`);
    if (gate.failed().length === 0) await suitesStage(ctx, gate);
    else for (const suite of SUITES) gate.row(`suite:${suite}`, 'BLOCKED', 'not launched: the cheap checks above are not clean (preflight.json)');
    const gains = gainsOf(ctx);
    if (gate.failed().length === 0) gate.row('gains', gains.total >= options.minGains ? 'PASS' : 'FAIL', `${gains.total} gains (${gains.suites} suite rows, ${gains.jetpack} Jetpack rows); --min-gains ${options.minGains}`);
    const plan = mergePlan(ctx);
    const verdict = gate.failed().length === 0 ? 'PASS' : 'FAIL';
    writeFileSync(join(work, 'REPORT.md'), report(ctx, gate, verdict, plan));
    for (const dir of [ctx.tree, ctx.releaseTree, join(work, 'pack')]) removeTree(dir);
    if (verdict !== 'PASS') {
        process.stdout.write(`FAIL: ${ctx.next} not released; ${seeds} is unchanged. ${gate.failed().map(row => `${row.check} ${row.status}`).join(', ')}. Report: ${join(work, 'REPORT.md')}\n`);
        return 1;
    }
    const released = assemble(ctx, gate, plan);
    say(`assembled ${released.dest}`);
    if (!options.filter) {
        const published = publish(ctx, released);
        if (!published.ok) {
            const why = `${ctx.next} not admitted: publication step "${published.step}" failed (${published.detail}); log ${published.log}`;
            if (!published.pushed) {
                if (published.tagged) git(['tag', '-d', ctx.next]);
                const aside = join(seeds, `.${ctx.next}.unpublished-${process.pid}`);
                renameSync(released.dest, aside);
                process.stdout.write(`FAIL: ${why}. ${seeds} has no ${ctx.next}: the assembled release moved to ${aside}.\n`);
            } else {
                process.stdout.write([
                    `FAIL: ${why}. Tag ${ctx.next} is on origin, but ${released.dest} is not admitted (no current.env): do not jet-use it.`,
                    `Once the cause is fixed, finish the publication from ${REPO}, then admit it:`,
                    ...published.left.map(command => `  ${command}`),
                    `  cp ${join(work, 'current.env')} ${released.dest}/current.env`, '',
                ].join('\n'));
            }
            return 1;
        }
    }
    copyFileSync(join(work, 'current.env'), join(released.dest, 'current.env'));
    printCommands(ctx, released, plan);
    return 0;
}

async function main() {
    const options = parseArgs(process.argv.slice(2));
    const pre = preconditions(options);
    if (pre.errors.length) {
        process.stderr.write(`${USAGE}\n${pre.errors.map(error => `release-compiler: ${error}`).join('\n')}\n`);
        return 2;
    }
    if (!pre.admission.admitted) {
        process.stderr.write(`${pre.admission.text}\n`);
        return 2;
    }
    if (options.checkGate) {
        process.stdout.write(`${pre.admission.text}\n`);
        return 0;
    }
    let lock = '';
    if (!process.env.JET_RELEASE_LOCK_HELD) {
        lock = takeLock(pre.workRoot);
        if (typeof lock !== 'string') {
            process.stderr.write(`release-compiler: another release run (pid ${lock.held || 'unknown'}) holds ${join(pre.workRoot, 'release.lock')}\n`);
            return 2;
        }
    }
    try {
        const slot = process.env.JET_RELEASE_SLOT ?? join(REPO, 'Tools/agent/chain/build-slot.sh');
        if (slot && existsSync(slot) && !process.env.SLOT_CPUS) {
            const inner = spawnSync('bash', [slot, process.execPath, SCRIPT, ...process.argv.slice(2)], { stdio: 'inherit', env: { ...process.env, JET_RELEASE_LOCK_HELD: '1' } });
            return inner.status ?? 1;
        }
        return await release(options, pre);
    } finally {
        if (lock) rmSync(lock, { recursive: true, force: true });
    }
}

if (process.argv[1] && resolve(process.argv[1]) === SCRIPT) process.exitCode = await main();
