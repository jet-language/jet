import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { runtimeExportSignatures } from './runtime-route-abi.mjs';
import { nativeEnvironmentRoutes, emitNativeEnvironmentRoutes } from './native-route-environment.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const registry = readFileSync(`${root}/Compiler/JetFoundation/Source/Registry/CoreCallRows.jet`, 'utf8');
const routes = new Map();
for (const match of registry.matchAll(/module_name: "([^"]+)", member: "([^"]+)".*symbol: CoreCallSymbol\.Prelude\{name: "([^"]+)"\}/g)) {
  const [, module, member, symbol] = match;
  const route = routes.get(symbol) ?? { rows: [] };
  route.rows.push(`${module}.${member}`);
  routes.set(symbol, route);
}
const signatures = runtimeExportSignatures(root);
const exports = new Set(signatures.keys());

test('real composed Core registry and Jet providers admit all six env primitives', () => {
  const rows = nativeEnvironmentRoutes(routes, exports, signatures);
  for (const name of ['jet_rt_logical_env_get_os', 'jet_rt_logical_env_set', 'jet_rt_logical_env_unset', 'jet_rt_logical_env_columns', 'jet_rt_startup_env_columns', 'jet_rt_env_key_compare']) {
    assert.equal(rows.filter(row => row.name === name).length, 1, `${name} requires fresh composed source/registry`);
  }
  assert.match(emitNativeEnvironmentRoutes(rows).join('\n'), /IR_ENVIRONMENT_ROUTES/);
});

test('absent real provider manifest ownership cannot generate a binding', () => {
  const without = new Set(exports);
  without.delete('jet_rt_logical_env_get_os');
  assert.equal(nativeEnvironmentRoutes(routes, without, signatures).some(row => row.name === 'jet_rt_logical_env_get_os'), false);
});

test('real provider definitions missing from source manifest are rejected', () => {
  assert(exports.has('jet_rt_logical_env_get_os'), 'requires composed logical environment provider');
  const without = new Map(signatures);
  without.delete('jet_rt_logical_env_get_os');
  assert.throws(() => nativeEnvironmentRoutes(routes, exports, without));
});
