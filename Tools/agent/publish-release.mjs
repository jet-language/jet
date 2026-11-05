#!/usr/bin/env node
// publish-release.mjs: build a compiler release from a dev commit. Tools/release (Jet) gates,
// records and publishes it as the next a0_NNN GitHub release.
//
//   node Tools/agent/publish-release.mjs build COMMIT --seed DIR --out DIR [--threads N]
//
// build exports COMMIT (it must be on dev) and runs the bootstrap chain of
// Docs/spec/contributing/bootstrap.md from the seed DIR, with no Rust on PATH: the seed builds
// s1, s1 packs its runtime and builds s2, s2 packs its runtime and builds s3, and s2 and s3 must
// be byte-identical. Every stage runs in the export as its working directory, so the images
// record source directories relative to it. OUT (new or empty, outside the checkout and /tmp)
// receives s2 as the release: jet, its aliases, jet-runtime/, IDENTITY (`<seed
// version>#<sha256 of jet>`), COMMIT, SEED (the seed's identity), BUILD.json (the runner
// target and the identities of s1, s2 and s3) and SHA256SUMS. A shipped file that names this
// machine's personal information (the rule table of Tools/tower/app/personal-info.mjs) or a
// build directory fails the build. Logs stay in OUT/logs.
//
// Then, in a checkout of COMMIT, once per runner (shard and target):
//   JET=OUT/jet JET_CI_EVIDENCE_DIR=EVDIR Tools/agent/verify-full.sh
//   jet run Tools/release/Main.jet -- candidate OUT --evidence EVDIR...
//   jet run Tools/release/Main.jet -- publish OUT
// `candidate` writes and signs OUT/CANDIDATE.json, the record that binds the release; `publish`
// is the one route to a GitHub release and refuses any release that no longer matches it.
//
// Environment: JET_PACK_MUSL_LIB (the static musl lib directory prebuild-runtime.mjs links).
// Exit: 0 done, 1 a check failed, 2 the run could not start.
import { spawnSync } from 'node:child_process';
import { chmodSync, copyFileSync, cpSync, existsSync, mkdirSync, readdirSync, readFileSync, realpathSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { machinePersonalInfo } from '../tower/app/personal-info.mjs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { MUSL_DEFAULT, NORUST, SEED_FILES, STACK, fileHashes, linkCompilerAliases, sha256File, sumsOf } from './release-compiler.mjs';

const SCRIPT = fileURLToPath(import.meta.url);
const REPO = resolve(dirname(SCRIPT), '../..');
const USAGE = 'usage: publish-release.mjs build COMMIT --seed DIR --out DIR [--threads N]';
// Where the stages find their tools: the export's jet-env, under the stack, with no Rust on PATH.
function stagedArgv(tree) {
    return [...STACK, join(tree, 'Tools/agent/jet-env'), 'bash', '-c', NORUST, 'norust'];
}

// ---- pure helpers (publish-release.test.mjs) ----

export function parseArgs(argv) {
    const options = { command: argv[0] ?? '', target: '', seed: '', out: '', threads: 8, errors: [] };
    const values = new Map([['--seed', 'seed'], ['--out', 'out']]);
    for (let i = 1; i < argv.length; i++) {
        const flag = argv[i];
        if (values.has(flag)) {
            const value = argv[++i];
            if (value === undefined) options.errors.push(`${flag} needs a value`);
            else options[values.get(flag)] = value;
        } else if (flag === '--threads') {
            const value = argv[++i] ?? '';
            if (!/^[1-9]\d*$/.test(value)) options.errors.push(`--threads needs a positive whole number, not ${value || 'nothing'}`);
            else options.threads = Number(value);
        } else if (flag.startsWith('-')) options.errors.push(`unknown option ${flag}`);
        else if (options.target) options.errors.push(`one commit, not ${options.target} and ${flag}`);
        else options.target = flag;
    }
    if (options.command !== 'build') { options.errors.push(`unknown command ${options.command || '(none)'}: want build (Tools/release/Main.jet gates and publishes)`); return options; }
    if (!options.target) options.errors.push('COMMIT is required');
    if (!options.seed) options.errors.push('--seed is required');
    if (!options.out) options.errors.push('--out is required');
    return options;
}

// What `bytes` names that no shipped file may: a span of the machine's personal information
// (`matcher`, Tools/tower/app/personal-info.mjs: home, user, host, mounts, git identity) or one
// of the directories the build read or wrote.
export function personalSpans(bytes, matcher, dirs) {
    const text = bytes.toString('latin1');
    return [...new Set([...matcher.spans(text).map(found => found.span), ...dirs.filter(dir => dir.length > 1 && text.includes(dir))])];
}

// ---- process helpers ----

function git(args) {
    const result = spawnSync('git', ['-C', REPO, ...args], { encoding: 'utf8', maxBuffer: 64 << 20 });
    return { rc: result.status, out: (result.stdout ?? '').trim(), err: (result.stderr ?? '').trim() };
}

function say(message) {
    process.stderr.write(`[publish-release ${new Date().toISOString().slice(11, 19)}] ${message}\n`);
}

// Runs argv with its output in `log`; returns the exit status (124 on timeout).
function run(argv, { cwd, env, log, timeout }) {
    const result = spawnSync(argv[0], argv.slice(1), { cwd, env, encoding: 'utf8', maxBuffer: 256 << 20, timeout: timeout * 1000, killSignal: 'SIGKILL' });
    writeFileSync(log, `${result.stdout ?? ''}${result.stderr ?? ''}${result.error ? `\n${result.error.message}\n` : ''}`);
    return result.error?.code === 'ETIMEDOUT' ? 124 : (result.status ?? 1);
}

function tail(path, lines = 6) {
    return existsSync(path) ? readFileSync(path, 'utf8').split('\n').filter(Boolean).slice(-lines).join(' | ').slice(0, 600) : '';
}

function onDev(sha, refs) {
    return refs.some(ref => git(['rev-parse', '--verify', '-q', ref]).rc === 0 && git(['merge-base', '--is-ancestor', sha, ref]).rc === 0);
}

// Every regular file of the shipped entries of `dir` that names personal information or one of `dirs`.
function personalFindings(dir, dirs) {
    const matcher = machinePersonalInfo();
    const findings = [];
    for (const name of SEED_FILES) {
        const path = join(dir, name);
        if (!existsSync(path)) continue;
        const files = statSync(path).isDirectory() ? [...fileHashes(path).keys()].map(rel => `${name}/${rel}`) : [name];
        for (const file of files) {
            const found = personalSpans(readFileSync(join(dir, file)), matcher, dirs);
            if (found.length) findings.push(`${file}: ${found.join(', ')}`);
        }
    }
    return findings;
}

// ---- build ----

function build(options) {
    const parsed = git(['rev-parse', '--verify', `${options.target}^{commit}`]);
    if (parsed.rc !== 0) return fail(2, `${options.target} is not a commit of ${REPO}`);
    const sha = parsed.out;
    if (!onDev(sha, ['refs/heads/dev', 'refs/remotes/origin/dev'])) return fail(2, `${sha.slice(0, 10)} is not on dev`);
    const seed = resolve(options.seed);
    const missing = ['jet', 'IDENTITY', 'jet-runtime'].filter(name => !existsSync(join(seed, name)));
    if (missing.length) return fail(2, `seed ${seed} lacks ${missing.join(', ')}`);
    const seedIdentity = readFileSync(join(seed, 'IDENTITY'), 'utf8').trim();
    const [version, seedSha] = seedIdentity.split('#');
    if (!/^jet@\S+$/.test(version) || seedSha !== sha256File(join(seed, 'jet'))) return fail(2, `seed IDENTITY ${seedIdentity} does not name the seed's jet (sha256 ${sha256File(join(seed, 'jet')).slice(0, 16)})`);
    const musl = process.env.JET_PACK_MUSL_LIB || MUSL_DEFAULT;
    if (!existsSync(join(musl, 'libc.a'))) return fail(2, `no musl libc.a in ${musl} (JET_PACK_MUSL_LIB)`);
    const out = resolve(options.out);
    if (existsSync(out) && readdirSync(out).length) return fail(2, `${out} is not empty`);
    const inside = (root, path) => { const part = relative(root, path); return part === '' || (!part.startsWith('..') && !part.startsWith('/')); };
    if (inside(REPO, out)) return fail(2, `${out} is inside the checkout`);
    mkdirSync(out, { recursive: true });
    if (inside(realpathSync('/tmp'), realpathSync(out))) return fail(2, `${out} is on RAM-backed /tmp; use a disk path`);

    const work = join(out, 'work'), logs = join(out, 'logs'), tree = join(work, 'tree'), tmp = join(work, 'tmp');
    for (const dir of [work, logs, tree, tmp]) mkdirSync(dir, { recursive: true });
    const archive = spawnSync('bash', ['-c', 'set -o pipefail; git -C "$0" archive "$1" | tar -x -C "$2"', REPO, sha, tree], { encoding: 'utf8' });
    if (archive.status !== 0) return fail(1, `git archive ${sha} failed: ${archive.stderr}`);
    const env = { ...process.env, JET_PACK_MUSL_LIB: musl, TMPDIR: tmp, JET_TOOLCHAIN_ROOT: tree };
    delete env.JET;
    const staged = stagedArgv(tree);
    say(`chain of ${sha.slice(0, 10)} from seed ${seedIdentity} in ${work}`);

    // `builder` (a directory with jet and jet-runtime/) builds the stage `name`, which then packs its runtime.
    const stage = (name, builder, identity, pack) => {
        const built = join(tree, 'Compiler/JetCli/.jet/build/jet');
        rmSync(dirname(built), { recursive: true, force: true });
        const started = Date.now();
        const rc = run([...staged, join(builder, 'jet'), 'build', '--threads', String(options.threads), 'Compiler'], { cwd: tree, env: { ...env, JET_COMPILER_IDENTITY: identity }, log: join(logs, `${name}.log`), timeout: 3600 });
        if (rc !== 0 || !existsSync(built)) return { error: `${name}: jet build Compiler rc=${rc}: ${tail(join(logs, `${name}.log`))}` };
        const dir = join(work, name);
        mkdirSync(dir, { recursive: true });
        copyFileSync(built, join(dir, 'jet'));
        chmodSync(join(dir, 'jet'), 0o755);
        const own = `${version}#${sha256File(join(dir, 'jet'))}`;
        say(`${name}: ${own} (${Math.round((Date.now() - started) / 1000)}s)`);
        if (!pack) return { dir, identity: own };
        const packRc = run([...staged, 'node', join(tree, 'Tools/packaging/prebuild-runtime.mjs'), join(dir, 'jet'), join(dir, 'jet-runtime'), '--repo', tree], { cwd: tree, env: { ...env, JET_COMPILER_IDENTITY: own }, log: join(logs, `${name}-pack.log`), timeout: 3600 });
        if (packRc !== 0 || !existsSync(join(dir, 'jet-runtime/native.key'))) return { error: `${name}: prebuild-runtime.mjs rc=${packRc}: ${tail(join(logs, `${name}-pack.log`))}` };
        return { dir, identity: own };
    };
    const s1 = stage('s1', seed, seedIdentity, true);
    if (s1.error) return fail(1, s1.error);
    const s2 = stage('s2', s1.dir, s1.identity, true);
    if (s2.error) return fail(1, s2.error);
    const s3 = stage('s3', s2.dir, s2.identity, false);
    if (s3.error) return fail(1, s3.error);
    if (s2.identity !== s3.identity) return fail(1, `no fixed point: s2 ${s2.identity} != s3 ${s3.identity}`);

    copyFileSync(join(s2.dir, 'jet'), join(out, 'jet'));
    chmodSync(join(out, 'jet'), 0o755);
    linkCompilerAliases(out);
    cpSync(join(s2.dir, 'jet-runtime'), join(out, 'jet-runtime'), { recursive: true, verbatimSymlinks: true });
    writeFileSync(join(out, 'IDENTITY'), `${s2.identity}\n`);
    writeFileSync(join(out, 'COMMIT'), `${sha}\n`);
    writeFileSync(join(out, 'SEED'), `${seedIdentity}\n`);
    // Tools/release reads the runner target and the chain's fixed point from here.
    writeFileSync(join(out, 'BUILD.json'), `${JSON.stringify({ target: `${process.platform}-${process.arch}`, chain: { s1: s1.identity, s2: s2.identity, s3: s3.identity } }, null, 2)}\n`);
    writeFileSync(join(out, 'SHA256SUMS'), sumsOf(out, SEED_FILES));
    const findings = personalFindings(out, [REPO, realpathSync(REPO), out, realpathSync(out), seed, realpathSync(seed)]);
    rmSync(work, { recursive: true, force: true });
    if (findings.length) return fail(1, `shipped files name personal information or build paths: ${findings.slice(0, 8).join('; ')}`);
    process.stdout.write([
        `BUILT ${out}: ${s2.identity} (s2 == s3) from ${sha}.`,
        'Gate it in a checkout of that commit, once per runner, then record and publish it:',
        `  JET=${join(out, 'jet')} JET_CI_EVIDENCE_DIR=<EVDIR> Tools/agent/verify-full.sh`,
        `  jet run Tools/release/Main.jet -- candidate ${out} --evidence <EVDIR>`,
        `  jet run Tools/release/Main.jet -- publish ${out} --dry-run`,
        '',
    ].join('\n'));
    return 0;
}

function fail(code, message) {
    process.stderr.write(`${code === 2 ? `${USAGE}\n` : ''}${message.split('\n').map(line => `publish-release: ${line}`).join('\n')}\n`);
    return code;
}

function main() {
    const options = parseArgs(process.argv.slice(2));
    if (options.errors.length) return fail(2, options.errors.join('\n'));
    return build(options);
}

if (process.argv[1] && resolve(process.argv[1]) === SCRIPT) process.exitCode = main();
