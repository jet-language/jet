#!/usr/bin/env node
// The exact audited exports/imports of the Jet object, not loose name prefixes.
// Expected exports: the hook's `<prefix>.exports` receipt (the checked
// NativeLibrary artifact rows), which must equal the runtime sources'
// `#Export(c)` functions.
import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [object, output] = process.argv.slice(2);
if (!object || !output || !object.endsWith('.o')) throw Error('usage: check-object.mjs <prefix>.o evidence-prefix');
const list = file => readFileSync(join(root, 'Compiler/JetRuntime', file), 'utf8').split('\n').map(row => row.trim()).filter(row => row && !row.startsWith('#')).map(row => row.split(/\s+/)[0]);
function inspect(bin, args, suffix) {
    const result = spawnSync(bin, args, { encoding: 'utf8' });
    writeFileSync(output + suffix, `${result.stdout ?? ''}${result.stderr ?? ''}`);
    if (result.error || result.status !== 0) throw Error(`${bin}: ${result.error ?? result.stderr}`);
    return result.stdout;
}
const failures = [];
const receiptPath = object.slice(0, -2) + '.exports';
if (!existsSync(receiptPath)) throw Error(`missing export receipt ${receiptPath}`);
const expected = readFileSync(receiptPath, 'utf8').split('\n').map(row => row.trim()).filter(Boolean);
const sourceDir = join(root, 'Compiler/JetRuntime/Source');
const annotated = [];
for (const file of readdirSync(sourceDir).filter(name => name.endsWith('.jet')).sort()) {
    const lines = readFileSync(join(sourceDir, file), 'utf8').split('\n').map(line => line.trim());
    lines.forEach((line, index) => {
        // `#Export(c) fn name(` on one line, or a standalone `#Export(c)`
        // line followed by the function (other markers and comments may sit
        // between them).
        if (!line.startsWith('#Export(c)')) return;
        const rest = line.slice('#Export(c)'.length).trim();
        const next = rest || lines.slice(index + 1).find(row => row && !row.startsWith('#') && !row.startsWith('//'));
        const name = /^(?:pub\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(/.exec(next ?? '')?.[1];
        if (!name) failures.push(`${file}:${index + 1}: #Export(c) is not followed by a function`);
        else annotated.push(name);
    });
}
for (const name of annotated) if (!expected.includes(name)) failures.push(`#Export(c) ${name} is missing from the checked export receipt`);
for (const name of expected) if (!annotated.includes(name)) failures.push(`receipt export ${name} has no #Export(c) source`);
if (new Set(expected).size !== expected.length) failures.push('duplicate receipt export');
const defined = inspect('nm', ['-g', '--defined-only', '--format=posix', object], '.exports.nm');
const actual = new Map(defined.trim().split('\n').filter(Boolean).map(row => row.split(/\s+/)));
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
