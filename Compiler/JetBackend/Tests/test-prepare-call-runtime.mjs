#!/usr/bin/env node
// Exercise the real adapter's process-context boundary, including bad inputs.
import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, input] = process.argv.slice(2);
if (!output || !input) throw Error('usage: test-prepare-call-runtime.mjs OUT emitted-runtime.rs');
const out = resolve(output);
mkdirSync(out, { recursive: true });
const source = readFileSync(input, 'utf8');
const declarations = [...source.matchAll(/^static JET_NATIVE_COMPTIME_ACTIVE:.*$/gm)];
if (declarations.length !== 1) throw Error('test input must have exactly one compiler-call context declaration');
const declaration = declarations[0][0];
const rejection = 'expected exactly one compiler-call context declaration in canonical and emitted runtime';
const failures = [];
const rows = [];
for (const [name, text, accepted] of [
  ['normal', source, true],
  ['missing', source.replace(declaration, ''), false],
  ['duplicate', source + '\n' + declaration + '\n', false],
]) {
  const fixture = join(out, `${name}.rs`);
  const prepared = join(out, `${name}.prepared.rs`);
  writeFileSync(fixture, text);
  const result = spawnSync(process.execPath, [join(repo, 'Compiler/JetBackend/Tests/prepare-call-runtime.mjs'), fixture, prepared, '--allparts'], { encoding: 'utf8' });
  writeFileSync(join(out, `${name}.stdout`), result.stdout ?? '');
  writeFileSync(join(out, `${name}.stderr`), result.stderr ?? '');
  writeFileSync(join(out, `${name}.status.json`), JSON.stringify({ status: result.status, signal: result.signal, error: result.error?.message ?? null }) + '\n');
  const pass = !result.error && !result.signal && (accepted
    ? result.status === 0 && [...readFileSync(prepared, 'utf8').matchAll(/^static JET_NATIVE_COMPTIME_ACTIVE:/gm)].length === 1
    : result.status !== 0 && result.stderr.includes(rejection));
  if (!pass) failures.push(name);
  rows.push(`${name}\t${pass ? 'PASS' : 'FAIL'}`);
}
writeFileSync(join(out, 'results.tsv'), rows.join('\n') + '\n');
console.log(rows.join('\n'));
if (failures.length) throw Error(`compiler-call context adapter failed: ${failures.join(', ')}`);
