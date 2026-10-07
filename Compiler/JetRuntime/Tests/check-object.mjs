#!/usr/bin/env node
// The exact audited exports/imports of the Jet object, not loose name prefixes.
import { spawnSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [object, output] = process.argv.slice(2);
if (!object || !output) throw Error('usage: check-object.mjs jet_runtime_jet.o evidence-prefix');
const list = file => readFileSync(join(root, 'Compiler/JetRuntime', file), 'utf8').split('\n').map(row => row.trim()).filter(row => row && !row.startsWith('#')).map(row => row.split(/\s+/)[0]);
function inspect(bin, args, suffix) {
    const result = spawnSync(bin, args, { encoding: 'utf8' });
    writeFileSync(output + suffix, `${result.stdout ?? ''}${result.stderr ?? ''}`);
    if (result.error || result.status !== 0) throw Error(`${bin}: ${result.error ?? result.stderr}`);
    return result.stdout;
}
const defined = inspect('nm', ['-g', '--defined-only', '--format=posix', object], '.exports.nm');
const actual = new Map(defined.trim().split('\n').filter(Boolean).map(row => row.split(/\s+/)));
const expected = list('exports.list');
const failures = [];
for (const name of expected) if (actual.get(name) !== 'T') failures.push(`${name}: expected T, got ${actual.get(name) ?? 'missing'}`);
for (const name of actual.keys()) if (!expected.includes(name)) failures.push(`unexpected definition ${name}`);
const imports = inspect('nm', ['-u', '--format=posix', object], '.imports.nm').trim().split('\n').filter(Boolean).map(row => row.trim().split(/\s+/)[0]);
const allowed = new Set(list('imports.list'));
for (const name of imports) if (!allowed.has(name)) failures.push(`unlisted import ${name}`);
const sections = inspect('readelf', ['-SW', object], '.sections');
if (!sections.includes('.init_array.00000')) failures.push('missing allocator init array');
const relocs = inspect('readelf', ['-rW', object], '.relocations');
if (!/R_X86_64_64\s+\S+\s+jet_rt_alloc_init/.test(relocs)) failures.push('missing allocator initializer relocation');
if (failures.length) throw Error(failures.join('\n'));
console.log(`Jet object: ${expected.length} exports, ${imports.length} audited imports, allocator constructor`);
