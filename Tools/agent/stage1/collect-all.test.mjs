import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
test('canonical sharded bootstrap backend requests every reachable Cargo error', () => {
  const source = readFileSync(new URL('../../../Compiler/Bootstrap/Tests.rs', import.meta.url), 'utf8');
  const start = source.indexOf('fn build_backend_artifact_with_native_library(');
  const end = source.indexOf('\nfn run_generated_artifact(', start);
  assert.ok(start >= 0 && end > start);
  const builder = source.slice(start, end);
  assert.match(builder, /\.args\(\[\s*"cargo",\s*"build",\s*"--keep-going",/);
});
