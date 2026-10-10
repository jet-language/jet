#!/usr/bin/env node
// Jet-only pack-linked widening fixture; no Rust rebuild or seed execution.
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import { spawnSync } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, ...inputs] = process.argv.slice(2);
if (!output || !inputs.length) throw Error('usage: run-int-widen.mjs OUT OBJECT_OR_ARCHIVE... [LINK_ARG...]');
const out = resolve(output);
mkdirSync(out, { recursive: true });
const binary = join(out, 'int-widen-fixture');
const built = spawnSync(nativeCCompiler, [
    '-std=c11', '-Wall', '-Wextra', '-Werror', '-pedantic',
    join(root, 'Compiler/JetRuntime/Tests/int-widen-fixture.c'),
    ...inputs.map(input => input.startsWith('-') ? input : resolve(input)), '-o', binary,
], { encoding: 'utf8' });
writeFileSync(join(out, 'build.log'), `${built.stdout ?? ''}${built.stderr ?? ''}`);
writeFileSync(join(out, 'build.status.json'), JSON.stringify({ status: built.status, signal: built.signal, error: built.error?.message ?? null }) + '\n');
if (built.error || built.signal || built.status !== 0) throw Error(`widening fixture link failed: ${built.error ?? built.signal ?? built.stderr}`);
const result = spawnSync(binary, [], { encoding: 'utf8' });
writeFileSync(join(out, 'stdout'), result.stdout ?? '');
writeFileSync(join(out, 'stderr'), result.stderr ?? '');
writeFileSync(join(out, 'status.json'), JSON.stringify({ status: result.status, signal: result.signal, error: result.error?.message ?? null }) + '\n');
const expected = 'CHECK OK — exact Int widening: precision24/53, signs, boxed/inline extremes, finite limits, borrowed file:u32-line\n';
if (result.error || result.signal || result.status !== 0 || result.stdout !== expected || result.stderr !== '') throw Error(`widening fixture failed: ${result.error ?? result.signal ?? result.stderr ?? result.status}`);
console.log('Jet-only exact Int widening C ABI fixture passed');
