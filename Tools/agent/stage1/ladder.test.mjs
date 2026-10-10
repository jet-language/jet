import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { lstatSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const sourceRoot = fileURLToPath(new URL('../../../', import.meta.url));
const ladder = fileURLToPath(new URL('ladder.mjs', import.meta.url));

test('real rung assembly leaves source authority single-linked and independently writable', () => {
  const directory = mkdtempSync(join(tmpdir(), 'jet-ladder-authority-'));
  const relative = 'Compiler/JetCodegen/Source/Codegen/AutoDerives.jet';
  const source = join(sourceRoot, relative);
  const before = lstatSync(source);
  const bytes = readFileSync(source);
  try {
    const assembled = spawnSync(process.execPath, [ladder, 'rungs', sourceRoot, directory], {
      cwd: sourceRoot, encoding: 'utf8', timeout: 120_000,
    });
    assert.equal(assembled.status, 0, `${assembled.error ?? ''}\n${assembled.stdout}\n${assembled.stderr}`);
    const staged = join(directory, 'L4/tree', relative);
    const copied = lstatSync(staged);
    const after = lstatSync(source);
    assert.equal(after.nlink, before.nlink, 'rung creation must not add links to package authority');
    assert.equal(copied.nlink, 1, 'the private rung is a single-link source too');
    assert.ok(copied.dev !== after.dev || copied.ino !== after.ino, 'rung and checkout need distinct file identities');
    assert.deepEqual(readFileSync(staged), bytes, 'copying preserves the real compiler source');
    writeFileSync(staged, '// an independent private rung edit\n');
    assert.deepEqual(readFileSync(source), bytes, 'rung edits cannot mutate checked source authority');
    assert.equal(lstatSync(source).ino, before.ino, 'staging must not replace the checkout file');
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
