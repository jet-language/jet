import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { ConfigError, loadConfig, publicConfig, saveConfig } from '../app/config.mjs';
import { configFile, writeJSON } from '../app/paths.mjs';

test('keyless config loads without reading the ignored legacy secrets file', () => {
  const dir = mkdtempSync(join(tmpdir(), 'tower-config-'));
  writeJSON(configFile(dir), { project: 'Split', port: 8123 });
  const legacySecrets = '{ intentionally invalid legacy data\n';
  const secrets = join(dir, 'secrets.json');
  writeFileSync(secrets, legacySecrets);

  const runtime = loadConfig(dir);
  assert.equal(runtime.project, 'Split');
  assert.equal(runtime.port, 8123);
  assert.equal(Object.hasOwn(runtime, 'auth'), false);
  assert.equal(Object.hasOwn(runtime, 'push'), false);
  assert.equal(readFileSync(secrets, 'utf8'), legacySecrets);

  const projected = publicConfig({ ...runtime, auth: { token: 'ignored' }, push: { old: true } });
  assert.equal(Object.hasOwn(projected, 'auth'), false);
  assert.equal(Object.hasOwn(projected, 'push'), false);
});

test('removed tracked secret fields fail with keyless migration guidance', () => {
  const dir = mkdtempSync(join(tmpdir(), 'tower-config-legacy-'));
  const marker = 'must-not-echo';
  writeJSON(configFile(dir), { project: 'Legacy', auth: { token: marker }, push: { privateJwk: { d: marker } } });
  assert.throws(() => loadConfig(dir), (error) => {
    assert.ok(error instanceof ConfigError);
    assert.match(error.message, /removed fields auth, push/);
    assert.match(error.message, /Remove auth\/push from tracked config/);
    assert.match(error.message, /does not load auth or push configuration/);
    assert.equal(error.message.includes(marker), false);
    return true;
  });
  assert.throws(() => saveConfig(dir, { auth: null }), ConfigError);
});
