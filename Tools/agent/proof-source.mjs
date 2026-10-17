// One immutable git-archive export per commit for all proof chains.
import { createHash } from 'node:crypto';
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, readlinkSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, isAbsolute, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const candidates = '/mnt/jetscratch/candidates';

export function proofTargetDirectory(commit) {
    if (!/^[a-f0-9]{40}$/.test(commit)) throw Error('proof targets require a full commit SHA');
    return `/mnt/jetscratch/targets/source-${commit}`;
}

function blobHash(path, mode) {
    const bytes = mode === '120000' ? readlinkSync(path, { encoding: 'buffer' }) : readFileSync(path);
    return createHash('sha1').update(`blob ${bytes.length}\0`).update(bytes).digest('hex');
}

export function assertPinnedSource(source, expected) {
    const manifest = JSON.parse(readFileSync(join(dirname(source), 'source.manifest.json'), 'utf8'));
    if (manifest.commit !== expected || manifest.schema !== 'jet-proof-source/v1' || !manifest.files.length) {
        throw Error(`proof source manifest is not pinned to ${expected}`);
    }
    for (const { path, mode, blob } of manifest.files) {
        if (isAbsolute(path) || path.split('/').includes('..')) throw Error(`invalid proof source path ${path}`);
        const file = join(source, path), metadata = lstatSync(file);
        if ((mode === '120000' && !metadata.isSymbolicLink()) ||
            (mode !== '120000' && (!metadata.isFile() || Boolean(metadata.mode & 0o111) !== (mode === '100755'))) ||
            blobHash(file, mode) !== blob) throw Error(`pinned proof source changed: ${path}`);
    }
    return manifest;
}

function git(repository, args) {
    const result = spawnSync('git', ['-C', repository, ...args], { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
    if (result.error || result.status !== 0) throw Error(`git ${args[0]} failed: ${result.error?.message ?? result.stderr}`);
    return result.stdout;
}

function completed(child, name) {
    return new Promise((accept, reject) => {
        child.once('error', reject);
        child.once('close', (status, signal) => status === 0 ? accept() : reject(Error(`${name} failed: ${status ?? signal}`)));
    });
}

async function exportSource(repository, expected) {
    const commit = git(repository, ['rev-parse', `${expected}^{commit}`]).trim();
    if (commit !== expected) throw Error('proof exports require a full commit SHA');
    mkdirSync(proofTargetDirectory(commit), { recursive: true });
    const destination = join(candidates, `source-${commit}`), source = join(destination, 'source');
    if (existsSync(destination)) {
        assertPinnedSource(source, commit);
        return source;
    }
    const temporary = mkdtempSync(`${destination}.tmp-`);
    mkdirSync(join(temporary, 'source'));
    try {
        const rows = git(repository, ['ls-tree', '-r', '-z', '--full-tree', commit]).split('\0').filter(Boolean);
        const files = rows.map((row) => {
            const at = row.indexOf('\t'), [mode, type, blob] = row.slice(0, at).split(' '), path = row.slice(at + 1);
            if (type !== 'blob' || !['100644', '100755', '120000'].includes(mode)) throw Error(`unsupported proof source entry ${path}: ${mode} ${type}`);
            return { path, mode, blob };
        });
        const archivePath = join(temporary, 'source.tar');
        const archive = spawn('git', ['-C', repository, 'archive', '--format=tar', `--output=${archivePath}`, commit],
            { stdio: 'inherit' });
        await completed(archive, 'git archive');
        const extract = spawn('tar', ['-xf', archivePath, '-C', join(temporary, 'source')], { stdio: 'inherit' });
        await completed(extract, 'tar');
        rmSync(archivePath);
        writeFileSync(join(temporary, 'source.manifest.json'), JSON.stringify({
            schema: 'jet-proof-source/v1', commit, tree: git(repository, ['rev-parse', `${commit}^{tree}`]).trim(), files,
        }) + '\n');
        assertPinnedSource(join(temporary, 'source'), commit);
        renameSync(temporary, destination);
        return source;
    } finally {
        rmSync(temporary, { recursive: true, force: true });
    }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
    const args = process.argv.slice(2);
    if (args[0] === '--locked') {
        if (args.length !== 3) throw Error('usage: proof-source.mjs SHA [REPOSITORY]');
        console.log(await exportSource(resolve(args[2]), args[1]));
    } else {
        if (args.length < 1 || args.length > 2) throw Error('usage: proof-source.mjs SHA [REPOSITORY]');
        mkdirSync(candidates, { recursive: true });
        const result = spawnSync('flock', ['--exclusive', join(candidates, '.source-export.lock'),
            process.execPath, fileURLToPath(import.meta.url), '--locked', args[0], resolve(args[1] ?? '.')], { stdio: 'inherit' });
        if (result.error) throw result.error;
        process.exitCode = result.status ?? 1;
    }
}
