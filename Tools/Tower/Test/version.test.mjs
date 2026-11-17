import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { computeVersion, sourceFiles } from '../App/version.mjs';

function fakeTowerRoot() {
  const dir = mkdtempSync(join(tmpdir(), 'tower-ver-'));
  mkdirSync(join(dir, 'App'));
  writeFileSync(join(dir, 'tower.mjs'), 'run();\n');
  writeFileSync(join(dir, 'App', 'server.mjs'), 'export const x = 1;\n');
  writeFileSync(join(dir, 'App', 'store.mjs'), 'export const y = 2;\n');
  mkdirSync(join(dir, 'App', 'UI'));
  writeFileSync(join(dir, 'App', 'UI', 'tower.js'), 'console.log(1);\n'); // never counted
  return dir;
}

test('sourceFiles: entry + App/*.mjs only, never App/UI', () => {
  const dir = fakeTowerRoot();
  const files = sourceFiles(dir);
  assert.equal(files.length, 3, 'tower.mjs + 2 App/*.mjs');
  assert.ok(files.every((f) => !f.includes(join('App', 'UI'))), 'App/UI excluded');
});

test('computeVersion: deterministic and changes with content', () => {
  const dir = fakeTowerRoot();
  const v1 = computeVersion(dir);
  const v2 = computeVersion(dir);
  assert.equal(v1, v2, 'pure function of file content — same input, same hash');

  writeFileSync(join(dir, 'App', 'server.mjs'), 'export const x = 999;\n');
  const v3 = computeVersion(dir);
  assert.notEqual(v1, v3, 'editing a watched source file changes the version');
});

test('computeVersion: touching App/UI files does not change version', () => {
  const dir = fakeTowerRoot();
  const v1 = computeVersion(dir);
  writeFileSync(join(dir, 'App', 'UI', 'tower.js'), 'console.log(2);\n');
  const v2 = computeVersion(dir);
  assert.equal(v1, v2, 'UI/ is served fresh off disk already — not part of the process-boot version');
});
