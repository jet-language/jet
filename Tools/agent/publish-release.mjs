#!/usr/bin/env node
// publish-release.mjs: build a compiler release from a dev commit, then publish it, once
// Tools/agent/verify-full.sh passes with that very binary, as the next a0_NNN GitHub release.
//
//   node Tools/agent/publish-release.mjs build COMMIT --seed DIR --out DIR [--threads N]
//   node Tools/agent/publish-release.mjs publish OUT --evidence DIR [--dry-run]
//
// build exports COMMIT (it must be on dev) and runs the bootstrap chain of
// Docs/spec/contributing/bootstrap.md from the seed DIR, with no Rust on PATH: the seed builds
// s1, s1 packs its runtime and builds s2, s2 packs its runtime and builds s3, and s2 and s3 must
// be byte-identical. Every stage runs in the export as its working directory, so the images
// record source directories relative to it. OUT (new or empty, outside the checkout and /tmp)
// receives s2 as the release: jet, its aliases, jet-runtime/, IDENTITY (`<seed
// version>#<sha256 of jet>`), COMMIT, SEED (the seed's identity) and SHA256SUMS. A shipped
// file that names this machine's personal information (the rule table of
// Tools/tower/app/personal-info.mjs) or a build directory fails the build. Logs stay in OUT/logs.
//
// The gate, between the two steps, in a checkout of COMMIT:
//   JET=OUT/jet JET_CI_EVIDENCE_DIR=EVDIR Tools/agent/verify-full.sh
//
// publish DIR is the one route to a GitHub release. DIR is either a build of this tool, gated
// by --evidence EVDIR (verify-full.json PASS, pinned to COMMIT, run with DIR/jet's bytes), or a
// release release-compiler.mjs admitted into SEEDS/a0_NNN, gated by its own RELEASE.json
// (verdict PASS, scope full, the same identity). A build needs COMMIT on origin's dev and takes
// the name one past the highest a0_NNN among the local tags, origin's tags and the GitHub
// releases; an admitted release keeps its name, whose tag must already be on origin at COMMIT.
// Either way the shipped files must match SHA256SUMS and name no personal information. It
// stages DIR/release/: the seed archive jet-a0_NNN-<platform>-<arch>.tar.gz (the seed files
// and SHA256SUMS) and its .sha256, jet, IDENTITY, SHA256SUMS of those, and NOTES.md (the full
// identity, the commit, the seed and the gate). Then it runs `gh release create a0_NNN
// --latest`, with --target COMMIT for a build (GitHub creates the tag) or --verify-tag for an
// admitted release. --dry-run stages everything and prints the command instead of running it.
//
// Environment: JET_PACK_MUSL_LIB (the static musl lib directory prebuild-runtime.mjs links).
// Exit: 0 done, 1 a check failed, 2 the run could not start.
import { spawnSync } from 'node:child_process';
import { chmodSync, copyFileSync, cpSync, existsSync, mkdirSync, readdirSync, readFileSync, realpathSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { machinePersonalInfo } from '../tower/app/personal-info.mjs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { MUSL_DEFAULT, NORUST, SEED_FILES, STACK, fileHashes, linkCompilerAliases, nextReleaseName, seedAssetName, sha256File, sumsOf } from './release-compiler.mjs';

const SCRIPT = fileURLToPath(import.meta.url);
const REPO = resolve(dirname(SCRIPT), '../..');
const USAGE = 'usage: publish-release.mjs build COMMIT --seed DIR --out DIR [--threads N]\n       publish-release.mjs publish DIR [--evidence EVDIR] [--dry-run]';
// Where the stages find their tools: the export's jet-env, under the stack, with no Rust on PATH.
function stagedArgv(tree) {
    return [...STACK, join(tree, 'Tools/agent/jet-env'), 'bash', '-c', NORUST, 'norust'];
}

// ---- pure helpers (publish-release.test.mjs) ----

export function parseArgs(argv) {
    const options = { command: argv[0] ?? '', target: '', seed: '', out: '', evidence: '', threads: 8, dryRun: false, errors: [] };
    const values = new Map([['--seed', 'seed'], ['--out', 'out'], ['--evidence', 'evidence']]);
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
        } else if (flag === '--dry-run') options.dryRun = true;
        else if (flag.startsWith('-')) options.errors.push(`unknown option ${flag}`);
        else if (options.target) options.errors.push(`one ${options.command === 'build' ? 'commit' : 'release directory'}, not ${options.target} and ${flag}`);
        else options.target = flag;
    }
    const allowed = { build: ['seed', 'out', 'threads'], publish: ['evidence', 'dryRun'] }[options.command];
    if (!allowed) { options.errors.push(`unknown command ${options.command || '(none)'}: want build or publish`); return options; }
    if (!options.target) options.errors.push(options.command === 'build' ? 'COMMIT is required' : 'OUT is required');
    if (options.command === 'build') {
        if (!options.seed) options.errors.push('--seed is required');
        if (!options.out) options.errors.push('--out is required');
        if (options.evidence || options.dryRun) options.errors.push('--evidence and --dry-run belong to publish');
    } else {
        if (options.seed || options.out || argv.includes('--threads')) options.errors.push('--seed, --out and --threads belong to build');
    }
    return options;
}

// What `bytes` names that no shipped file may: a span of the machine's personal information
// (`matcher`, Tools/tower/app/personal-info.mjs: home, user, host, mounts, git identity) or one
// of the directories the build read or wrote.
export function personalSpans(bytes, matcher, dirs) {
    const text = bytes.toString('latin1');
    return [...new Set([...matcher.spans(text).map(found => found.span), ...dirs.filter(dir => dir.length > 1 && text.includes(dir))])];
}

// owner/name of a GitHub remote URL (https or ssh), or ''.
export function githubSlug(url) {
    const match = /github\.com[:/]([^/\s]+)\/([^/\s]+?)(?:\.git)?\/?$/.exec(url.trim());
    return match ? `${match[1]}/${match[2]}` : '';
}

// Each tag `git ls-remote --tags` prints, with the commit it names (the peeled `^{}` row of an
// annotated tag wins over the tag object).
export function remoteTags(text) {
    const tags = new Map();
    for (const line of text.split('\n')) {
        const match = /^([0-9a-f]+)\trefs\/tags\/([^\s^]+)(\^\{\})?$/.exec(line);
        if (match && (match[3] || !tags.has(match[2]))) tags.set(match[2], match[1]);
    }
    return tags;
}

// What publish requires of verify-full.json: a PASS pinned to `commit`, run with the release's jet.
export function evidenceProblems(evidence, commit, jetSha, shaOf) {
    const problems = [];
    if (evidence.status !== 'PASS') problems.push(`status is ${evidence.status ?? 'missing'}, not PASS`);
    const failed = (evidence.rows ?? []).filter(row => row.status !== 'PASS');
    if (failed.length) problems.push(`${failed.length} row(s) not PASS: ${failed.slice(0, 4).map(row => `${row.scope} ${row.status}`).join(', ')}`);
    if (!(evidence.rows ?? []).length) problems.push('no rows: the gate ran nothing');
    if (evidence.head !== commit) problems.push(`pinned to ${evidence.head ?? 'nothing'}, not ${commit}`);
    const gated = evidence.jet ? shaOf(evidence.jet) : '';
    if (gated !== jetSha) problems.push(`gated ${evidence.jet ?? 'no compiler'} (${gated ? gated.slice(0, 16) : 'unreadable'}), not the release's jet (${jetSha.slice(0, 16)})`);
    return problems;
}

// What publish requires of the RELEASE.json release-compiler.mjs admits a release with: a full,
// passing gate of the release's own identity.
export function admissionProblems(record, identity) {
    const problems = [];
    if (record.schema !== 'jet-release/v1') problems.push(`schema is ${record.schema ?? 'missing'}, not jet-release/v1`);
    if (record.verdict !== 'PASS') problems.push(`verdict is ${record.verdict ?? 'missing'}, not PASS`);
    if (record.scope !== 'full') problems.push(`scope is ${record.scope ?? 'missing'}, not full: a filtered gate is not a release`);
    if (record.identity !== identity) problems.push(`admitted ${record.identity ?? 'no identity'}, not ${identity}`);
    if (!/^a0_\d{3}$/.test(record.name ?? '') || !/^[0-9a-f]{40}$/.test(record.commit ?? '')) problems.push('name or commit is missing');
    return problems;
}

export function releaseNotes({ name, identity, commit, seed, gate, slug, asset }) {
    return [
        `${name}: the jet compiler of dev ${commit.slice(0, 10)}.`,
        '',
        `- Identity: \`${identity}\``,
        `- Commit: ${commit}`,
        `- Seed: \`${seed}\`; the chain from it reaches s2 == s3 with no Rust on PATH, and this release is s2.`,
        `- Gate: ${gate}.`,
        '',
        `\`${asset}\` is a seed directory (Docs/spec/contributing/bootstrap.md). Fetch the latest release with:`,
        '',
        '```sh',
        `gh release download -R ${slug} -p 'jet-a0_*-linux-x64.tar.gz*' && sha256sum -c jet-a0_*-linux-x64.tar.gz.sha256 && mkdir seed && tar -xzf jet-a0_*-linux-x64.tar.gz -C seed && (cd seed && sha256sum -c SHA256SUMS)`,
        '```',
        '',
    ].join('\n');
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
    writeFileSync(join(out, 'SHA256SUMS'), sumsOf(out, SEED_FILES));
    const findings = personalFindings(out, [REPO, realpathSync(REPO), out, realpathSync(out), seed, realpathSync(seed)]);
    rmSync(work, { recursive: true, force: true });
    if (findings.length) return fail(1, `shipped files name personal information or build paths: ${findings.slice(0, 8).join('; ')}`);
    process.stdout.write([
        `BUILT ${out}: ${s2.identity} (s2 == s3) from ${sha}.`,
        'Gate it in a checkout of that commit, then publish:',
        `  JET=${join(out, 'jet')} JET_CI_EVIDENCE_DIR=<EVDIR> Tools/agent/verify-full.sh`,
        `  node Tools/agent/publish-release.mjs publish ${out} --evidence <EVDIR> --dry-run`,
        '',
    ].join('\n'));
    return 0;
}

// ---- publish ----

function publish(options) {
    const out = resolve(options.target);
    const admitted = existsSync(join(out, 'RELEASE.json'));
    const required = ['jet', 'jet-runtime', 'IDENTITY', 'SHA256SUMS', ...(admitted ? [] : ['COMMIT', 'SEED'])];
    const missing = required.filter(name => !existsSync(join(out, name)));
    if (missing.length) return fail(2, `${out} lacks ${missing.join(', ')}: not a publish-release.mjs build or a release-compiler.mjs release`);
    if (admitted === Boolean(options.evidence)) return fail(2, admitted ? `${out} is admitted by its RELEASE.json; drop --evidence` : 'a build needs --evidence EVDIR (its verify-full.sh run)');
    const identity = readFileSync(join(out, 'IDENTITY'), 'utf8').trim();
    const jetSha = sha256File(join(out, 'jet'));
    const problems = [];
    if (identity.split('#')[1] !== jetSha) problems.push(`IDENTITY ${identity} does not name jet (${jetSha.slice(0, 16)})`);
    if (readFileSync(join(out, 'SHA256SUMS'), 'utf8') !== sumsOf(out, SEED_FILES)) problems.push('SHA256SUMS does not match the shipped files');
    let commit, seed, gate, tag = '';
    if (admitted) {
        const record = JSON.parse(readFileSync(join(out, 'RELEASE.json'), 'utf8'));
        problems.push(...admissionProblems(record, identity).map(problem => `RELEASE.json: ${problem}`));
        ({ commit = '', name: tag = '' } = record);
        seed = record.seed?.identity ?? '';
        gate = `\`Tools/agent/release-compiler.mjs\` admitted it from seed ${record.seed?.name ?? '?'} (${(record.checks ?? []).length} checks, ${record.gains?.total ?? 0} rows gained)`;
    } else {
        commit = readFileSync(join(out, 'COMMIT'), 'utf8').trim();
        seed = readFileSync(join(out, 'SEED'), 'utf8').trim();
        const evidencePath = join(resolve(options.evidence), 'verify-full.json');
        if (!existsSync(evidencePath)) problems.push(`${evidencePath} does not exist`);
        else {
            const evidence = JSON.parse(readFileSync(evidencePath, 'utf8'));
            const shaOf = path => { try { return sha256File(path); } catch { return ''; } };
            problems.push(...evidenceProblems(evidence, commit, jetSha, shaOf).map(problem => `verify-full: ${problem}`));
            gate = `\`Tools/agent/verify-full.sh\` passed on this commit with this binary (${(evidence.rows ?? []).length} checks)`;
        }
        if (!onDev(commit, ['refs/remotes/origin/dev'])) problems.push(`${commit.slice(0, 10)} is not on origin/dev: push dev first, so GitHub has the commit the tag names`);
    }
    const slug = githubSlug(git(['remote', 'get-url', 'origin']).out);
    if (!slug) problems.push('origin is not a GitHub remote');
    const remote = spawnSync('git', ['-C', REPO, 'ls-remote', '--tags', 'origin', 'a0_*'], { encoding: 'utf8' });
    if (remote.status !== 0) problems.push(`git ls-remote origin failed: ${(remote.stderr ?? '').trim()}`);
    const tags = remoteTags(remote.stdout ?? '');
    const listed = slug ? spawnSync('gh', ['release', 'list', '-R', slug, '--limit', '1000', '--json', 'tagName', '--jq', '.[].tagName'], { encoding: 'utf8' }) : { status: 1, stderr: '' };
    if (slug && listed.status !== 0) problems.push(`gh release list failed: ${(listed.stderr ?? '').trim()}`);
    const released = (listed.stdout ?? '').split('\n').filter(Boolean);
    if (tag && remote.status === 0 && tags.get(tag) !== commit) problems.push(`origin's tag ${tag} ${tags.has(tag) ? `names ${tags.get(tag).slice(0, 10)}` : 'is missing'}, not ${commit.slice(0, 10)}: push the tag first`);
    if (tag && released.includes(tag)) problems.push(`${tag} is already a GitHub release`);
    const machine = personalFindings(out, [REPO, realpathSync(REPO)]);
    if (machine.length) problems.push(`shipped files name personal information or build paths: ${machine.slice(0, 8).join('; ')}`);
    if (problems.length) return fail(1, problems.join('\n'));

    const name = tag || nextReleaseName([...git(['tag', '-l', 'a0_*']).out.split('\n'), ...tags.keys(), ...released].filter(Boolean));
    const stage = join(out, 'release');
    rmSync(stage, { recursive: true, force: true });
    mkdirSync(stage);
    const asset = seedAssetName(name);
    const packed = spawnSync('tar', ['--sort=name', '--owner=0', '--group=0', '--numeric-owner', '--mtime=@0', '--use-compress-program=gzip -n', '-cf', join(stage, asset), ...SEED_FILES.filter(file => existsSync(join(out, file))), 'SHA256SUMS'], { cwd: out, encoding: 'utf8' });
    if (packed.status !== 0) return fail(1, `packing ${asset} failed: ${packed.stderr}`);
    writeFileSync(join(stage, `${asset}.sha256`), `${sha256File(join(stage, asset))}  ${asset}\n`);
    copyFileSync(join(out, 'jet'), join(stage, 'jet'));
    chmodSync(join(stage, 'jet'), 0o755);
    copyFileSync(join(out, 'IDENTITY'), join(stage, 'IDENTITY'));
    writeFileSync(join(stage, 'SHA256SUMS'), sumsOf(stage, [asset, 'jet', 'IDENTITY']));
    writeFileSync(join(stage, 'NOTES.md'), releaseNotes({ name, identity, commit, seed, gate, slug, asset }));
    const assets = [asset, `${asset}.sha256`, 'jet', 'IDENTITY', 'SHA256SUMS'];
    const argv = ['gh', 'release', 'create', name, '-R', slug, ...(tag ? ['--verify-tag'] : ['--target', commit]), '--title', name, '--notes-file', 'NOTES.md', '--latest', ...assets];
    const shown = argv.map(word => (/^[\w@%+=:,./-]+$/.test(word) ? word : `'${word.replace(/'/g, `'\\''`)}'`)).join(' ');
    if (options.dryRun) {
        process.stdout.write(`DRY RUN: ${name} staged in ${stage} (${identity}, commit ${commit}). To publish, run from ${stage}:\n  ${shown}\nor rerun without --dry-run.\n`);
        return 0;
    }
    say(`publishing ${name}: ${shown}`);
    const created = spawnSync(argv[0], argv.slice(1), { cwd: stage, stdio: 'inherit' });
    if (created.status !== 0) return fail(1, `gh release create failed (rc ${created.status}); ${stage} is kept`);
    process.stdout.write(`PUBLISHED ${name} on ${slug} at ${commit}. Fetch the tag with: git fetch origin tag ${name}\n`);
    return 0;
}

function fail(code, message) {
    process.stderr.write(`${code === 2 ? `${USAGE}\n` : ''}${message.split('\n').map(line => `publish-release: ${line}`).join('\n')}\n`);
    return code;
}

function main() {
    const options = parseArgs(process.argv.slice(2));
    if (options.errors.length) return fail(2, options.errors.join('\n'));
    return options.command === 'build' ? build(options) : publish(options);
}

if (process.argv[1] && resolve(process.argv[1]) === SCRIPT) process.exitCode = main();
