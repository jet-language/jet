#!/usr/bin/env node
// Compose all private Jet crypto KAT exports into ONE candidate native object.
// C files transport fixtures only. Explicit phases follow the runtime witness
// harness convention; no fixture can fall back to a Rust crypto implementation.
import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync, existsSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { candidateJetc0, frontReceipt } from '../../JetBackend/Tests/jet-front.mjs';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const [output, mode] = process.argv.slice(2);
if (!output || !['assemble', 'lower', 'audit', 'link', 'run'].includes(mode)) throw Error('usage: run-crypto-fixtures.mjs OUT assemble|lower|audit|link|run');
const out = resolve(output), unit = join(out, 'unit');
const fixtures = ['Sha256', 'CryptoX25519', 'CryptoEd25519', 'CryptoChaCha', 'CryptoAead', 'CryptoHkdf', 'CryptoArgon', 'AgeCodec', 'AgeV1'];
function command(name, binary, args, options = {}) {
    const result = spawnSync(binary, args, {cwd: root, encoding: 'utf8', maxBuffer: 1 << 27, ...options});
    writeFileSync(join(out, `${name}.receipt.json`), JSON.stringify({status: result.status, signal: result.signal, error: result.error?.message ?? null, stdout: result.stdout, stderr: result.stderr}, null, 2) + '\n');
    if (result.error || result.status !== 0) throw Error(`${name}: ${result.error?.message ?? result.status}\n${result.stdout ?? ''}${result.stderr ?? ''}`);
    return result;
}
if (mode === 'assemble') {
    mkdirSync(join(unit, 'Source'), {recursive: true});
    const paths = readFileSync(join(root, 'Compiler/JetRuntime/sources.list'), 'utf8').split('\n').map(x => x.trim()).filter(x => x && !x.startsWith('#'));
    paths.push(...fixtures.map(name => `Compiler/JetRuntime/Tests/${name}Tests.jet`));
    const imports = new Set();
    const bodies = paths.map(path => {
        let importing = false;
        const lines = readFileSync(join(root, path), 'utf8').split('\n').filter(line => {
            if (line.startsWith('use ')) {
                if (line.startsWith('use core.')) imports.add(line);
                importing = line.includes('[') && !line.includes(']');
                return false;
            }
            if (importing) { if (line.includes(']')) importing = false; return false; }
            return true;
        });
        return `// [unit source: ${path}]\n${lines.join('\n')}`;
    });
    writeFileSync(join(unit, 'Source/Fixture.jet'), [...imports, '', ...bodies].join('\n'));
    writeFileSync(join(unit, 'package.jet'), readFileSync(join(root, 'Compiler/JetRuntime/package.jet')));
    const entries = readFileSync(join(root, 'Compiler/JetRuntime/exports.list'), 'utf8').split('\n').filter(x => x && !x.startsWith('#'));
    for (const name of fixtures) for (const match of readFileSync(join(root, `Compiler/JetRuntime/Tests/${name}Tests.jet`), 'utf8').matchAll(/#Export\(c\)\s+fn\s+(\w+)\(/gu)) entries.push(match[1]);
    writeFileSync(join(unit, 'exports.list'), entries.sort().join('\n') + '\n');
    writeFileSync(join(out, 'sources.receipt.json'), JSON.stringify({paths}, null, 2) + '\n');
} else if (mode === 'lower') {
    const prefix = join(unit, 'fixture'), receipt = join(unit, 'front.receipt');
    const result = command('front', candidateJetc0(), [], {timeout: Number(process.env.JET_FRONT_TIMEOUT_MS ?? 600000), env: {...process.env,
        JET_BOOTSTRAP_MODE: 'factory', JET_BOOTSTRAP_FACTORY_TIER: 'aot', JET_BOOTSTRAP_SOURCE_ROOT: unit,
        JET_BOOTSTRAP_ENTRY: 'Source/Fixture.jet', JET_BOOTSTRAP_OUTPUT: join(unit, 'unused.output'), JET_BOOTSTRAP_RECEIPT: receipt,
        JET_FRONT_LIBRARY_OBJECT_OUTPUT: prefix, JET_FRONT_LIBRARY_EXPORTS: join(unit, 'exports.list'), JET_STORE_DIR: join(out, 'store'), RUST_MIN_STACK: '8388608'}});
    const problem = frontReceipt(existsSync(receipt) ? readFileSync(receipt, 'utf8') : '', result);
    if (problem) throw Error(`candidate front ${problem.verdict}: ${problem.detail}`);
    if (existsSync(prefix + '.issues')) throw Error(readFileSync(prefix + '.issues', 'utf8'));
    if (!existsSync(prefix + '.o')) throw Error('candidate did not emit the real crypto fixture object');
} else if (mode === 'audit') {
    const object = join(unit, 'fixture.o');
    const imports = command('imports', 'nm', ['-u', '--format=posix', object]).stdout;
    const allowed = new Set(readFileSync(join(root, 'Compiler/JetRuntime/imports.list'), 'utf8').split('\n').map(x => x.trim()).filter(x => x && !x.startsWith('#')));
    for (const line of imports.trim().split('\n').filter(Boolean)) if (!allowed.has(line.split(/\s+/u)[0])) throw Error(`unledgered import ${line}`);
    const definitions = command('definitions', 'nm', ['--defined-only', '--format=posix', object]).stdout;
    const strong = new Set(definitions.trim().split('\n').filter(Boolean).filter(line => line.split(/\s+/u)[1] === 'T').map(line => line.split(/\s+/u)[0]));
    for (const name of fixtures) for (const match of readFileSync(join(root, `Compiler/JetRuntime/Tests/${name}Tests.jet`), 'utf8').matchAll(/#Export\(c\)\s+fn\s+(\w+)\(/gu)) if (!strong.has(match[1])) throw Error(`missing strong Jet fixture export ${match[1]}`);
    command('disassembly', 'objdump', ['-dr', object]);
} else if (mode === 'link') {
    const library = process.env.JET_RUNTIME_C_LIB;
    if (!library || !existsSync(library)) throw Error('fresh composed JET_RUNTIME_C_LIB required');
    for (const name of fixtures) command(`link-${name}`, process.env.CC ?? 'cc', ['-std=c11', '-Wall', '-Wextra', '-Werror', join(root, `Compiler/JetRuntime/Tests/${name}Witness.c`), join(unit, 'fixture.o'), library, '-lpthread', '-ldl', '-lm', '-o', join(out, name)]);
} else {
    const digest = bytes => createHash('sha256').update(bytes).digest('hex');
    const fixtureFile = join(root, 'Compiler/JetRuntime/Tests/AgeV1Fixtures.json');
    const frozen = JSON.parse(readFileSync(fixtureFile, 'utf8'));
    const material = join(out, 'age-material');
    mkdirSync(material, {recursive: true});
    const captures = join(out, 'jet-seals');
    mkdirSync(captures, {recursive: true});
    for (const name of fixtures.filter(name => name !== 'AgeV1')) command(`run-${name}`, join(out, name), [], {timeout: name === 'CryptoArgon' ? 300000 : 60000});
    for (const test of frozen.cases) {
        const cipher = Buffer.from(test.cipher, 'base64');
        if (digest(cipher) !== test.cipherSha256) throw Error(`damaged frozen vector ${test.name}`);
        const path = join(material, `${test.name}.age`);
        writeFileSync(path, cipher);
        command(`run-${test.name}`, join(out, 'AgeV1'), ['open', test.identity, path, test.valid ? '1' : '0', test.plainSha256 ?? '-'], {timeout: 60000});
    }
    for (const test of frozen.seals) {
        const cipher = Buffer.from(test.cipher, 'base64');
        if (digest(cipher) !== test.cipherSha256) throw Error(`damaged frozen seal ${test.name}`);
        const plainPath = join(material, `${test.name}.plain`), cipherPath = join(material, `${test.name}.age`), capturePath = join(captures, `${test.name}.age`);
        writeFileSync(plainPath, Buffer.from(test.plain, 'base64'));
        writeFileSync(cipherPath, cipher);
        command(`run-${test.name}`, join(out, 'AgeV1'), ['seal', test.identity, plainPath, cipherPath, String(test.recipientCount), capturePath], {timeout: 60000});
        if (!readFileSync(capturePath).equals(cipher)) throw Error(`Jet capture differs ${test.name}`);
    }
    console.log(`CRYPTO_NATIVE_KAT_OK primitive_witnesses=${fixtures.length - 1} age_open=${frozen.cases.length} age_seal=${frozen.seals.length} fixtures_sha256=${digest(readFileSync(fixtureFile))}`);
    console.log(`ONE_TIME_RUST_OPENER_OWED captures=${captures}`);
}
