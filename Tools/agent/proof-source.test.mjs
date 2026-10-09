import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { chmodSync, mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
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
