import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { computeVersion, sourceFiles } from '../app/version.mjs';

function fakeTowerRoot() {
  const dir = mkdtempSync(join(tmpdir(), 'tower-ver-'));
  mkdirSync(join(dir, 'app'));
  writeFileSync(join(dir, 'tower.mjs'), 'run();\n');
  writeFileSync(join(dir, 'app', 'server.mjs'), 'export const x = 1;\n');
  writeFileSync(join(dir, 'app', 'store.mjs'), 'export const y = 2;\n');
  mkdirSync(join(dir, 'app', 'ui'));
  writeFileSync(join(dir, 'app', 'ui', 'tower.js'), 'console.log(1);\n'); // never counted
  return dir;
}

test('sourceFiles: entry + app/*.mjs only, never app/ui', () => {
  const dir = fakeTowerRoot();
  const files = sourceFiles(dir);
  assert.equal(files.length, 3, 'tower.mjs + 2 app/*.mjs');
  assert.ok(files.every((f) => !f.includes(join('app', 'ui'))), 'app/ui excluded');
});

test('computeVersion: deterministic and changes with content', () => {
  const dir = fakeTowerRoot();
  const v1 = computeVersion(dir);
  const v2 = computeVersion(dir);
  assert.equal(v1, v2, 'pure function of file content — same input, same hash');

  writeFileSync(join(dir, 'app', 'server.mjs'), 'export const x = 999;\n');
  const v3 = computeVersion(dir);
  assert.notEqual(v1, v3, 'editing a watched source file changes the version');
});

test('computeVersion: touching app/ui files does not change version', () => {
  const dir = fakeTowerRoot();
  const v1 = computeVersion(dir);
  writeFileSync(join(dir, 'app', 'ui', 'tower.js'), 'console.log(2);\n');
  const v2 = computeVersion(dir);
  assert.equal(v1, v2, 'ui/ is served fresh off disk already — not part of the process-boot version');
});
