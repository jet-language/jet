import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  chmodSync, lstatSync, mkdirSync, mkdtempSync, readFileSync,
  readdirSync, statSync, symlinkSync, writeFileSync,
} from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { ConfigError, loadConfig, publicConfig, saveConfig, saveSecrets } from '../app/config.mjs';
import { configFile, readJSON, secretsFile, writeJSON } from '../app/paths.mjs';

test('public config and runtime secrets load through separate files', () => {
  const dir = mkdtempSync(join(tmpdir(), 'tower-config-'));
  writeJSON(configFile(dir), { project: 'Split', port: 8123 });
  saveSecrets(dir, { auth: { token: 'runtime-only' }, push: { publicKey: 'public-shape', privateJwk: { d: 'private-shape' }, subscriptions: [] } });

  const runtime = loadConfig(dir);
  assert.equal(runtime.project, 'Split');
  assert.equal(typeof runtime.auth?.token, 'string');
  assert.equal(typeof runtime.push?.privateJwk, 'object');
  assert.deepEqual(Object.keys(readJSON(configFile(dir), {})).sort(), ['port', 'project']);
  assert.deepEqual(Object.keys(publicConfig(runtime)).sort().includes('auth'), false);
  assert.deepEqual(Object.keys(publicConfig(runtime)).sort().includes('push'), false);
  assert.equal(statSync(secretsFile(dir)).mode & 0o777, 0o600);
});

test('tracked secret fields fail with migration and rotation guidance', () => {
  const dir = mkdtempSync(join(tmpdir(), 'tower-config-legacy-'));
  const marker = 'must-not-echo';
  writeJSON(configFile(dir), { project: 'Legacy', auth: { token: marker }, push: { privateJwk: { d: marker } } });
  assert.throws(() => loadConfig(dir), (error) => {
    assert.ok(error instanceof ConfigError);
    assert.match(error.message, /\.tower\/secrets\.json/);
    assert.match(error.message, /rotate/);
    assert.match(error.message, /will not load tracked secrets/);
    assert.equal(error.message.includes(marker), false);
    return true;
  });
  assert.throws(() => saveConfig(dir, { auth: null }), ConfigError);
});

test('secret loading rejects links, special files, and broad permissions before parsing', () => {
  const linkedDir = mkdtempSync(join(tmpdir(), 'tower-config-link-'));
  writeJSON(configFile(linkedDir), { project: 'Linked' });
  const outside = join(linkedDir, 'outside.json');
  writeFileSync(outside, '{}\n', { mode: 0o600 });
  symlinkSync(outside, secretsFile(linkedDir));
  assert.throws(() => loadConfig(linkedDir), (error) => {
    assert.ok(error instanceof ConfigError);
    assert.match(error.message, /regular file/);
    return true;
  });

  const directoryDir = mkdtempSync(join(tmpdir(), 'tower-config-directory-'));
  writeJSON(configFile(directoryDir), { project: 'Directory' });
  mkdirSync(secretsFile(directoryDir));
  assert.throws(() => loadConfig(directoryDir), /regular file/);

  const modeDir = mkdtempSync(join(tmpdir(), 'tower-config-mode-'));
  writeJSON(configFile(modeDir), { project: 'Mode' });
  writeFileSync(secretsFile(modeDir), 'intentionally invalid JSON');
  chmodSync(secretsFile(modeDir), 0o640);
  assert.throws(() => loadConfig(modeDir), (error) => {
    assert.ok(error instanceof ConfigError);
    assert.match(error.message, /permissions are too broad/);
    assert.equal(error.message.includes('valid JSON'), false, 'mode is rejected before parsing');
    return true;
  });
});

test('exclusive random secret temp cannot be redirected by precreated links', () => {
  const dir = mkdtempSync(join(tmpdir(), 'tower-config-temp-attack-'));
  writeJSON(configFile(dir), { project: 'Temp Attack' });
  const decoy = join(dir, 'decoy');
  writeFileSync(decoy, 'unchanged\n', { mode: 0o600 });
  const before = readFileSync(decoy);
  const legacyTemp = `${secretsFile(dir)}.tmp.${process.pid}`;
  const guessedTemp = join(dir, '.secrets.json.tmp-00000000000000000000000000000000');
  symlinkSync(decoy, legacyTemp);
  symlinkSync(decoy, guessedTemp);

  saveSecrets(dir, { auth: { token: 'generated-test-shape' } });

  assert.equal(readFileSync(decoy).equals(before), true, 'redirect target unchanged');
  assert.equal(lstatSync(legacyTemp).isSymbolicLink(), true);
  assert.equal(lstatSync(guessedTemp).isSymbolicLink(), true);
  assert.equal(lstatSync(secretsFile(dir)).isFile(), true);
  assert.equal(statSync(secretsFile(dir)).mode & 0o777, 0o600);
  const randomTemps = readdirSync(dir).filter(name =>
    name.startsWith('.secrets.json.tmp-')
      && name !== '.secrets.json.tmp-00000000000000000000000000000000');
  assert.deepEqual(randomTemps, [], 'writer leaves no random temporary files');
});
