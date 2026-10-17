#!/usr/bin/env node
// Real Jet-only object/archive input; no seed runtime or Rust oracle fallback.
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import { spawnSync } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, ...inputs] = process.argv.slice(2);
if (!output || !inputs.length) throw Error('usage: run-compiler-needed-math.mjs OUT JET_ONLY_OBJECT_OR_ARCHIVE... [LINK_ARG...]');
const out = resolve(output);
mkdirSync(out, { recursive: true });
const binary = join(out, 'compiler-needed-math-fixture');
const built = spawnSync(nativeCCompiler, [
  '-std=c11', '-Wall', '-Wextra', '-Werror', '-pedantic', '-fno-builtin',
  join(root, 'Compiler/JetRuntime/Tests/compiler-needed-math-fixture.c'),
  ...inputs.map(input => input.startsWith('-') ? input : resolve(input)),
  '-lpthread', '-ldl', '-lm', '-o', binary,
], { encoding: 'utf8' });
writeFileSync(join(out, 'build.log'), `${built.stdout ?? ''}${built.stderr ?? ''}`);
writeFileSync(join(out, 'build.status.json'), JSON.stringify({ status: built.status, signal: built.signal, error: built.error?.message ?? null }) + '\n');
if (built.error || built.signal || built.status !== 0) throw Error(`compiler math fixture link failed: ${built.error ?? built.signal ?? built.stderr}`);
const result = spawnSync(binary, [], { encoding: 'utf8' });
writeFileSync(join(out, 'stdout'), result.stdout ?? '');
writeFileSync(join(out, 'stderr'), result.stderr ?? '');
writeFileSync(join(out, 'status.json'), JSON.stringify({ status: result.status, signal: result.signal, error: result.error?.message ?? null }) + '\n');
const expected = 'CHECK OK — Jet-only compiler math: bit-exact platform libm, payloads, zeros, domains, fused cancellation\n';
if (result.error || result.signal || result.status !== 0 || result.stdout !== expected || result.stderr !== '') throw Error(`compiler math fixture failed: ${result.error ?? result.signal ?? result.stderr ?? result.status}`);
console.log('Jet-only compiler math C ABI fixture passed');
