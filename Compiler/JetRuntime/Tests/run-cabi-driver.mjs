#!/usr/bin/env node
// Explicit pack-linked C-ABI test. Missing object/provider fails, never skips.
import { spawnSync } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, rlib, jetObject] = process.argv.slice(2);
if (!output || !rlib || !jetObject) throw Error('usage: run-cabi-driver.mjs OUT fresh-native.rlib jet_runtime_jet.o');
const out = resolve(output);
mkdirSync(out, { recursive: true });
const driver = join(out, 'driver');
const built = spawnSync(process.env.RUSTC ?? 'rustc', ['--edition', '2021', '--cfg', 'jet_native_pack', join(repo, 'tests/fixtures/runtime_c_abi_driver.rs'), '--extern', `jet_runtime=${resolve(rlib)}`, '-C', `link-arg=${resolve(jetObject)}`, '-o', driver], { encoding: 'utf8' });
writeFileSync(join(out, 'build.log'), `${built.stdout ?? ''}${built.stderr ?? ''}`);
if (built.error || built.status !== 0) throw Error(`driver link failed: ${built.error ?? built.stderr}`);
const context = '  --> main.jet:3 in run\n    |\n3 |     #require_eq(total, 2)\n    |     ^^^^^^^^^^^^^^^^^^^^^^^\n';
const cases = [
    ['values', 0, '-4\n1\n"a\\\"b\\n"\n', '', true],
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
