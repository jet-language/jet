#!/usr/bin/env node
// Explicit Jet-only pack-linked C-ABI test. Missing object/provider fails.
import { nativeCCompiler } from '../../../Tools/packaging/native-c-compiler.mjs';
import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, ...inputs] = process.argv.slice(2);
if (!output || !inputs.some(input => !input.startsWith('-'))) throw Error('usage: run-cabi-driver.mjs OUT JET_ONLY_OBJECT_OR_ARCHIVE... [LINK_ARG...]');
const out = resolve(output);
mkdirSync(out, { recursive: true });
const driver = join(out, 'driver');
// Supply real Jet-only objects/archives (KEY/jet_runtime_jet.o, not the startup
// pack jet_runtime.pack.o); unresolved exports fail, never use a Rust provider.
const built = spawnSync(nativeCCompiler, ['-std=c11', '-Wall', '-Wextra', '-Werror', '-pedantic',
    join(repo, 'Compiler/JetRuntime/Tests/CAbiDriver.c'),
    ...inputs.map(input => input.startsWith('-') ? input : resolve(input)), '-lpthread', '-ldl', '-lm', '-o', driver], { encoding: 'utf8' });
writeFileSync(join(out, 'build.log'), `${built.stdout ?? ''}${built.stderr ?? ''}`);
writeFileSync(join(out, 'build.status.json'), JSON.stringify({ status: built.status, signal: built.signal, error: built.error?.message ?? null }) + '\n');
if (built.error || built.signal || built.status !== 0) throw Error(`driver link failed: ${built.error ?? built.signal ?? built.stderr}`);
const context = '  --> main.jet:3 in run\n    |\n3 |     #require_eq(total, 2)\n    |     ^^^^^^^^^^^^^^^^^^^^^^^\n';
const cases = [
    ['values', 0, readFileSync(join(repo, 'Compiler/JetRuntime/Tests/CAbiDriver.stdout'), 'utf8'), '', true],
    ['text-carriers', 0, '', '', true],
    ['euclid-by-zero', 70, '', 'Stop [E3010]: `divided by zero`\n  --> main.jet:6\n', false],
    ['numeric-stop', 70, '', "Stop [E3010]: `This addition overflows the value's type (the result is outside its range)`\n  --> main.jet:7\n", false],
    ['require', 70, '', `Stop [E3001]: \`panic: condition failed\`\n${context}locals: total = 1\n`, false],
    ['require-eq', 70, '', `Stop [E3001]: \`panic: expected: 2, got: 1\`\n${context} Why:`, false],
    ['panic', 70, '', `Stop [E3001]: \`panic: boom\`\n${context} Why:`, false],
];
const failures = [];
for (const [name, code, stdout, stderr, exact] of cases) {
    const result = spawnSync(driver, [name], { encoding: 'utf8' });
    writeFileSync(join(out, `${name}.stdout`), result.stdout ?? '');
    writeFileSync(join(out, `${name}.stderr`), result.stderr ?? '');
    writeFileSync(join(out, `${name}.status.json`), JSON.stringify({ status: result.status, signal: result.signal, error: result.error?.message ?? null }) + '\n');
    if (result.error || result.signal || result.status !== code || result.stdout !== stdout || (exact ? result.stderr !== stderr : !result.stderr.startsWith(stderr))) failures.push(name);
}
if (failures.length) throw Error(`pack C-ABI cases failed: ${failures.join(', ')}`);
console.log(`${cases.length} pack-linked C-ABI cases passed`);
