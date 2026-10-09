import assert from 'node:assert/strict';
import { createHash, randomBytes } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { assertPinnedSource, proofTargetDirectory } from './proof-source.mjs';

function fixture(t) {
    const directory = mkdtempSync(join(tmpdir(), 'jet-proof-source-'));
    t.after(() => rmSync(directory, { recursive: true, force: true }));
    const source = join(directory, 'source'), commit = 'a'.repeat(40), text = 'source bytes\n';
    mkdirSync(source);
    writeFileSync(join(source, 'entry.jet'), text);
    chmodSync(join(source, 'entry.jet'), 0o644);
    symlinkSync('entry.jet', join(source, 'alias.jet'));
    const blob = (bytes) => createHash('sha1').update(`blob ${Buffer.byteLength(bytes)}\0`).update(bytes).digest('hex');
    writeFileSync(join(directory, 'source.manifest.json'), JSON.stringify({ schema: 'jet-proof-source/v1', commit, files: [
        { path: 'entry.jet', mode: '100644', blob: blob(text) },
        { path: 'alias.jet', mode: '120000', blob: blob('entry.jet') },
    ] }));
    return { source, commit };
}

test('manifest guard accepts unchanged tracked bytes and symlink targets', (t) => {
    const { source, commit } = fixture(t);
    assert.equal(assertPinnedSource(source, commit).commit, commit);
});

test('manifest guard rejects another proof pin', (t) => {
    const { source } = fixture(t);
    assert.throws(() => assertPinnedSource(source, 'b'.repeat(40)), /manifest is not pinned/);
});

test('manifest guard rejects tracked source changes', (t) => {
    const { source, commit } = fixture(t);
    writeFileSync(join(source, 'entry.jet'), 'changed\n');
    assert.throws(() => assertPinnedSource(source, commit), /pinned proof source changed: entry.jet/);
});

test('manifest guard rejects executable-mode changes', (t) => {
    const { source, commit } = fixture(t);
    chmodSync(join(source, 'entry.jet'), 0o755);
    assert.throws(() => assertPinnedSource(source, commit), /pinned proof source changed: entry.jet/);
});

test('manifest guard rejects changed symlink targets', (t) => {
    const { source, commit } = fixture(t);
    rmSync(join(source, 'alias.jet'));
    symlinkSync('elsewhere.jet', join(source, 'alias.jet'));
    assert.throws(() => assertPinnedSource(source, commit), /pinned proof source changed: alias.jet/);
});

test('manifest guard rejects missing tracked files', (t) => {
    const { source, commit } = fixture(t);
    rmSync(join(source, 'entry.jet'));
    assert.throws(() => assertPinnedSource(source, commit), /ENOENT/);
});

test('proof targets use the full immutable commit rather than a moving branch', () => {
    const commit = 'a'.repeat(40);
    assert.equal(proofTargetDirectory(commit), `/mnt/jetscratch/targets/source-${commit}`);
    assert.throws(() => proofTargetDirectory('dev'), /full commit SHA/);
    assert.throws(() => proofTargetDirectory(commit.slice(0, 9)), /full commit SHA/);
});

test('export extracts a completed tar file and publishes once with no archive left behind', (t) => {
    // A command fixture avoids creating any Git branch/ref for this proof.
    const fixture = mkdtempSync(join(tmpdir(), 'jet-proof-archive-'));
    const commit = randomBytes(20).toString('hex'), tree = 'c'.repeat(40);
    const destination = `/mnt/jetscratch/candidates/source-${commit}`;
    const target = proofTargetDirectory(commit);
    assert.equal(existsSync(destination), false);
    assert.equal(existsSync(target), false);
    t.after(() => {
        rmSync(fixture, { recursive: true, force: true });
        rmSync(destination, { recursive: true, force: true });
        rmSync(target, { recursive: true, force: true });
    });
    const input = join(fixture, 'input'), bin = join(fixture, 'bin'), log = join(fixture, 'commands.jsonl');
    mkdirSync(input); mkdirSync(bin);
    const text = 'archive payload\n'.repeat(65537);
    writeFileSync(join(input, 'entry.jet'), text);
    chmodSync(join(input, 'entry.jet'), 0o755);
    symlinkSync('entry.jet', join(input, 'alias.jet'));
    const blob = bytes => createHash('sha1').update(`blob ${Buffer.byteLength(bytes)}\0`).update(bytes).digest('hex');
    const rows = `100755 blob ${blob(text)}\tentry.jet\0` + `120000 blob ${blob('entry.jet')}\talias.jet\0`;
    writeFileSync(join(bin, 'git'), `#!/usr/bin/env node
const fs = require('node:fs'), {spawnSync} = require('node:child_process');
const args = process.argv.slice(2);
fs.appendFileSync(${JSON.stringify(log)}, JSON.stringify(args) + '\\n');
if (args[2] === 'rev-parse') process.stdout.write((args[3].endsWith('^{tree}') ? ${JSON.stringify(tree)} : ${JSON.stringify(commit)}) + '\\n');
else if (args[2] === 'ls-tree') process.stdout.write(${JSON.stringify(rows)});
else if (args[2] === 'archive') {
  const output = args.find(arg => arg.startsWith('--output='));
  if (!output || !args.includes('--format=tar')) throw Error('archive must be completed on disk before extraction');
  const result = spawnSync('tar', ['-cf', output.slice('--output='.length), '-C', ${JSON.stringify(input)}, 'entry.jet', 'alias.jet'], {stdio:'inherit'});
  process.exitCode = result.status ?? 1;
} else throw Error('unexpected git command');
`);
    chmodSync(join(bin, 'git'), 0o755);
    const command = [join(dirname(fileURLToPath(import.meta.url)), 'proof-source.mjs'), commit, fixture];
    const run = () => spawnSync(process.execPath, command, {encoding:'utf8', env:{...process.env, PATH:`${bin}:${process.env.PATH}`}});
    const first = run();
    assert.equal(first.status, 0, first.stderr);
    const source = first.stdout.trim();
    assert.equal(source, join(destination, 'source'));
    assert.equal(assertPinnedSource(source, commit).tree, tree);
    assert.equal(readFileSync(join(source, 'entry.jet'), 'utf8'), text);
    assert.equal(existsSync(join(destination, 'source.tar')), false);
    const second = run();
    assert.equal(second.status, 0, second.stderr);
    assert.equal(second.stdout.trim(), source);
    const commands = readFileSync(log, 'utf8').trim().split('\n').map(line => JSON.parse(line));
    assert.equal(commands.filter(args => args[2] === 'archive').length, 1);
});
